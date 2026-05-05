//! End-to-end MEV signing test against a mock Flashbots relay that
//! performs the **same** `ecrecover` validation the real Flashbots relay
//! performs. This is the highest-confidence assertion possible without
//! actually hitting `relay.flashbots.net`: if the daemon's signature
//! survives `ecrecover` and matches the claimed signer address, the
//! production relay will accept it too.
//!
//! Closes the gap that mockito leaves: it returns canned bytes and
//! cannot validate the signature header against the request body.
//! The fixture below is built with the same `secp256k1` primitives the
//! daemon uses to sign, but reads the signature back via
//! `recover_ecdsa` — exactly Flashbots' verification path.
//!
//! ## Why this matters
//!
//! The Phase-1 bug — `sign_ecdsa` instead of `sign_ecdsa_recoverable` —
//! produced a 65-byte payload with a **bogus** recovery byte. The
//! signature still parsed, the daemon still claimed success, the daemon's
//! own auth.rs unit tests still passed. The only thing that catches this
//! class of bug is the actual recovery operation. That's what this file
//! does, on every test run, no real internet required.

use std::net::SocketAddr;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use axum::{
    extract::State,
    http::{HeaderMap, StatusCode},
    routing::post,
    Router,
};
use axum_test::TestServer;
use secp256k1::ecdsa::{RecoverableSignature, RecoveryId};
use secp256k1::{Message, Secp256k1};
use serde_json::{json, Value};
use sha3::{Digest, Keccak256};
use tokio::net::TcpListener;

use torpc::app::{build_app, AppConfig};
use torpc::mev::FlashbotsAuthenticator;

/// State threaded through the mock relay handler.
#[derive(Clone)]
struct RelayState {
    /// The address the daemon must successfully recover to. If the
    /// recovered address differs (wrong key, wrong message, broken sig),
    /// the relay rejects with 401 — this is what real Flashbots does.
    expected_address: String,
    /// Counter — `>0` after the daemon has actually called us. Use to
    /// distinguish "daemon never sent" from "daemon sent and was accepted".
    accepted_calls: Arc<AtomicU64>,
    /// Counter for rejected calls (signature didn't ecrecover correctly).
    rejected_calls: Arc<AtomicU64>,
}

/// Verify a Flashbots-style signature header against the body. This is
/// the EIP-191 `ecrecover` path; any deviation in the daemon's signing
/// flow (wrong message format, missing recovery byte, wrong hash) will
/// fail somewhere here. Returns `Some(recovered_address)` on success.
fn verify_flashbots_signature(header: &str, body: &str) -> Option<String> {
    // Header format: "0xAddress:0xSignature"
    let (claimed_address, sig_hex) = header.split_once(':')?;
    let sig_bytes = hex::decode(sig_hex.trim_start_matches("0x")).ok()?;
    if sig_bytes.len() != 65 {
        return None;
    }

    // Reconstruct the EIP-191 hash the daemon should have signed.
    let prefix = format!("\x19Ethereum Signed Message:\n{}", body.len());
    let mut prefixed = prefix.into_bytes();
    prefixed.extend_from_slice(body.as_bytes());
    let hash = {
        let mut h = Keccak256::new();
        h.update(&prefixed);
        h.finalize()
    };
    let message = Message::from_slice(&hash).ok()?;

    // Decode v: Flashbots uses Ethereum convention v = recovery_id + 27.
    let v = sig_bytes[64].checked_sub(27)?;
    let recovery_id = RecoveryId::from_i32(v as i32).ok()?;
    let recoverable = RecoverableSignature::from_compact(&sig_bytes[..64], recovery_id).ok()?;

    let secp = Secp256k1::new();
    let public_key = secp.recover_ecdsa(&message, &recoverable).ok()?;
    let pk_bytes = public_key.serialize_uncompressed();
    let mut h = Keccak256::new();
    h.update(&pk_bytes[1..]); // strip the 0x04 prefix
    let pk_hash = h.finalize();
    let recovered = format!("0x{}", hex::encode(&pk_hash[12..]));

    // Header's claimed address must match the recovered address.
    if recovered.eq_ignore_ascii_case(claimed_address) {
        Some(recovered)
    } else {
        None
    }
}

/// Mock relay handler. Accepts the bundle iff the signature recovers to
/// the configured `expected_address`. Returns a real-looking bundleHash
/// on success, 401 on any signature failure.
async fn relay_handler(
    State(state): State<Arc<RelayState>>,
    headers: HeaderMap,
    body: String,
) -> (StatusCode, axum::Json<Value>) {
    let header_value = match headers
        .get("x-flashbots-signature")
        .and_then(|v| v.to_str().ok())
    {
        Some(s) => s.to_string(),
        None => {
            state.rejected_calls.fetch_add(1, Ordering::SeqCst);
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(json!({"error": "missing X-Flashbots-Signature"})),
            );
        }
    };

    let recovered = match verify_flashbots_signature(&header_value, &body) {
        Some(addr) => addr,
        None => {
            state.rejected_calls.fetch_add(1, Ordering::SeqCst);
            return (
                StatusCode::UNAUTHORIZED,
                axum::Json(json!({"error": "signature verification failed"})),
            );
        }
    };

    if !recovered.eq_ignore_ascii_case(&state.expected_address) {
        state.rejected_calls.fetch_add(1, Ordering::SeqCst);
        return (
            StatusCode::UNAUTHORIZED,
            axum::Json(json!({
                "error": format!(
                    "signer mismatch: recovered={}, expected={}",
                    recovered, state.expected_address
                )
            })),
        );
    }

    state.accepted_calls.fetch_add(1, Ordering::SeqCst);
    (
        StatusCode::OK,
        axum::Json(json!({
            "jsonrpc": "2.0",
            "result": {"bundleHash": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"},
            "id": 1,
        })),
    )
}

/// Spin up the mock relay on an ephemeral port. Returns its URL plus the
/// shared counters so tests can assert exact accept/reject counts.
async fn spawn_mock_relay(
    expected_address: String,
) -> (
    String,
    Arc<AtomicU64>,
    Arc<AtomicU64>,
    tokio::task::JoinHandle<()>,
) {
    let accepted = Arc::new(AtomicU64::new(0));
    let rejected = Arc::new(AtomicU64::new(0));
    let state = Arc::new(RelayState {
        expected_address,
        accepted_calls: accepted.clone(),
        rejected_calls: rejected.clone(),
    });

    let app = Router::new()
        .route("/", post(relay_handler))
        .with_state(state);
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let url = format!("http://{addr}");

    let handle = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });
    // Give axum a tick to install the service.
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;

    (url, accepted, rejected, handle)
}

/// Standalone unit test for the relay's signature verifier. Establishes
/// the verifier itself works against a known-good signature produced by
/// the daemon's own `FlashbotsAuthenticator` — separately from the e2e
/// path, so a failure of the e2e test isn't muddied by a bug in the
/// fixture's verifier.
#[test]
fn relay_verifier_accepts_correctly_signed_payload() {
    let key = "1111111111111111111111111111111111111111111111111111111111111111";
    let auth = FlashbotsAuthenticator::new(key).unwrap();
    let body = r#"{"jsonrpc":"2.0","method":"eth_sendBundle","params":[],"id":1}"#;
    let header = auth.sign_request(body).unwrap();

    let recovered = verify_flashbots_signature(&header, body)
        .expect("verifier must accept a daemon-produced signature");
    assert_eq!(recovered, auth.address());
}

/// Counterpart: tampering with the body must invalidate the signature.
#[test]
fn relay_verifier_rejects_tampered_body() {
    let key = "1111111111111111111111111111111111111111111111111111111111111111";
    let auth = FlashbotsAuthenticator::new(key).unwrap();
    let body = r#"{"jsonrpc":"2.0","method":"eth_sendBundle","params":[],"id":1}"#;
    let header = auth.sign_request(body).unwrap();

    // Mutate one byte of the body — the recovered address will no longer
    // match the address in the header, so verification returns `None`.
    let tampered = body.replace("eth_sendBundle", "eth_sendXundle");
    assert!(
        verify_flashbots_signature(&header, &tampered).is_none(),
        "verifier must reject when body has been tampered with"
    );
}

/// THE e2e test. Spin up:
///   - mock Geth (current block lookup)
///   - mock Flashbots relay (does real ecrecover)
/// Configure the daemon with a known signing key, send an
/// `eth_sendRawTransaction` through `/rpc/flashbots`. The daemon builds a
/// bundle, signs it, sends to the mock relay. The relay verifies the
/// signature via ecrecover, returns a real bundleHash. The daemon
/// forwards that hash back to the wallet.
///
/// If anything in the daemon's signing flow regresses (wrong message
/// format, wrong message hashing, wrong recovery byte), the relay
/// returns 401 and this test fails.
#[tokio::test]
async fn daemon_signed_bundle_passes_relay_ecrecover() {
    let signing_key = "1111111111111111111111111111111111111111111111111111111111111111";
    let auth = FlashbotsAuthenticator::new(signing_key).unwrap();
    let signer_address = auth.address().to_string();

    let (relay_url, accepted, rejected, relay_handle) =
        spawn_mock_relay(signer_address.clone()).await;

    let mut geth = mockito::Server::new_async().await;
    let geth_mock = geth
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","result":"0x100","id":1}"#)
        .expect_at_least(1)
        .create_async()
        .await;

    // Build the daemon's router with the mock relay + signing key.
    let mut config = AppConfig::for_testing(geth.url());
    config.mev_signing_key = Some(signing_key.to_string());
    config.mev_relay_url = relay_url.clone();
    let built = build_app(config).await.unwrap();
    let server = TestServer::new(built.app).unwrap();

    let response = server
        .post("/rpc/flashbots")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_sendRawTransaction",
            "params": ["0x02f86b0180808252089400000000000000000000000000000000000000008080c001a0d91cd92cd079e3c9e22be2ce33a32e0b3eee02a3e039c7c39c3bf12a4b9b4fa01234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"],
            "id": 99,
        }))
        .await;

    assert_eq!(
        response.status_code(),
        200,
        "daemon must succeed when the relay accepts the signature"
    );
    let body: Value = response.json();
    let returned = body["result"].as_str().expect("result must be a string");
    assert!(
        returned.starts_with("0x") && returned.len() == 66,
        "daemon must return a real-looking 32-byte bundle hash, got {returned:?}"
    );
    assert_ne!(
        returned, "0x0000000000000000000000000000000000000000000000000000000000000000",
        "regression: daemon must NOT return the all-zero placeholder bundleHash"
    );
    assert_eq!(body["id"], 99, "JSON-RPC id must be echoed");

    // Strict assertion: relay actually accepted exactly one signed bundle.
    assert_eq!(accepted.load(Ordering::SeqCst), 1, "relay must accept once");
    assert_eq!(
        rejected.load(Ordering::SeqCst),
        0,
        "relay must not reject any"
    );
    geth_mock.assert_async().await;

    relay_handle.abort();
}

/// Negative test: configure the daemon with a DIFFERENT signing key from
/// the address the relay expects. The daemon's signature recovers to the
/// wrong address, the relay rejects, and the daemon returns an error to
/// the wallet. Proves the relay's check is actually doing work — and
/// that the daemon doesn't silently swallow auth failures.
#[tokio::test]
async fn daemon_signing_with_wrong_key_is_rejected_by_relay() {
    let expected_key = "1111111111111111111111111111111111111111111111111111111111111111";
    let expected_signer = FlashbotsAuthenticator::new(expected_key)
        .unwrap()
        .address()
        .to_string();

    // Daemon will sign with key 22…22, which derives a different address.
    let actual_key = "2222222222222222222222222222222222222222222222222222222222222222";
    let actual_signer = FlashbotsAuthenticator::new(actual_key)
        .unwrap()
        .address()
        .to_string();
    assert_ne!(expected_signer, actual_signer, "test setup sanity");

    let (relay_url, accepted, rejected, relay_handle) = spawn_mock_relay(expected_signer).await;

    let mut geth = mockito::Server::new_async().await;
    let _g = geth
        .mock("POST", "/")
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(r#"{"jsonrpc":"2.0","result":"0x100","id":1}"#)
        .expect_at_least(0)
        .create_async()
        .await;

    let mut config = AppConfig::for_testing(geth.url());
    config.mev_signing_key = Some(actual_key.to_string());
    config.mev_relay_url = relay_url.clone();
    let built = build_app(config).await.unwrap();
    let server = TestServer::new(built.app).unwrap();

    let response = server
        .post("/rpc/flashbots")
        .json(&json!({
            "jsonrpc": "2.0",
            "method": "eth_sendRawTransaction",
            "params": ["0x02f86b0180808252089400000000000000000000000000000000000000008080c001a0d91cd92cd079e3c9e22be2ce33a32e0b3eee02a3e039c7c39c3bf12a4b9b4fa01234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"],
            "id": 1,
        }))
        .await;

    // The daemon's MEV client wraps the relay error and returns a non-
    // success status. We don't pin the exact status code (the retry
    // policy may change it) — what matters is the relay rejected and
    // the wallet didn't receive a faked-success bundle hash.
    assert_ne!(
        response.status_code(),
        200,
        "wrong signing key must NOT produce a 200 (the relay must reject)"
    );

    // Strict: the relay must have rejected at least once. Daemon retry
    // policy may have caused multiple attempts; we don't assert exact count.
    assert_eq!(accepted.load(Ordering::SeqCst), 0, "relay must accept zero");
    assert!(
        rejected.load(Ordering::SeqCst) >= 1,
        "relay must reject at least once when the daemon signs with the wrong key"
    );

    relay_handle.abort();
}
