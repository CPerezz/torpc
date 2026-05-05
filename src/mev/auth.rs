//! Flashbots authentication using EIP-191 signatures
//!
//! This module implements the X-Flashbots-Signature authentication scheme
//! required for submitting bundles to Flashbots relays.

use secp256k1::{Message, Secp256k1, SecretKey};
use sha3::{Digest, Keccak256};
use std::sync::Arc;
use thiserror::Error;

/// Errors that can occur during authentication
#[derive(Error, Debug)]
pub enum AuthError {
    /// Invalid private key format
    #[error("Invalid private key: {0}")]
    InvalidKey(String),

    /// Signature generation failed
    #[error("Failed to sign message: {0}")]
    SigningError(String),

    /// Hex encoding/decoding error
    #[error("Hex encoding error: {0}")]
    HexError(#[from] hex::FromHexError),
}

/// Handles Flashbots authentication using EIP-191 signatures
///
/// # Security Note
/// The signing key is used only for authentication, not for transactions.
/// Any Ethereum key can be used - it doesn't need to hold funds.
///
/// # For Developers  
/// The authenticator signs the entire JSON-RPC request body to prove
/// the request hasn't been tampered with in transit.
///
/// # Example
/// ```rust
/// let auth = FlashbotsAuthenticator::new("your-private-key-hex")?;
/// let body = r#"{"jsonrpc":"2.0","method":"eth_sendBundle",...}"#;
/// let signature_header = auth.sign_request(body)?;
/// // Use signature_header as X-Flashbots-Signature
/// ```
#[derive(Clone)]
pub struct FlashbotsAuthenticator {
    /// Secp256k1 context for signing
    secp: Arc<Secp256k1<secp256k1::All>>,
    /// Private key for signing
    signing_key: SecretKey,
    /// Public address derived from signing key (0x-prefixed)
    signer_address: String,
}

impl FlashbotsAuthenticator {
    /// Create a new authenticator with the given private key
    ///
    /// # Arguments
    /// * `private_key_hex` - Private key in hex format (with or without 0x prefix)
    ///
    /// # Returns
    /// * `Ok(FlashbotsAuthenticator)` - Configured authenticator
    /// * `Err(AuthError)` - If the private key is invalid
    pub fn new(private_key_hex: &str) -> Result<Self, AuthError> {
        let secp = Arc::new(Secp256k1::new());

        // Remove 0x prefix if present
        let key_hex = private_key_hex
            .strip_prefix("0x")
            .unwrap_or(private_key_hex);

        // Parse private key
        let key_bytes = hex::decode(key_hex)?;
        let signing_key =
            SecretKey::from_slice(&key_bytes).map_err(|e| AuthError::InvalidKey(e.to_string()))?;

        // Derive public key and address
        let public_key = signing_key.public_key(&secp);
        let public_key_bytes = public_key.serialize_uncompressed();

        // Ethereum address is last 20 bytes of keccak256(public_key)
        let mut hasher = Keccak256::new();
        hasher.update(&public_key_bytes[1..]); // Skip the 0x04 prefix
        let hash = hasher.finalize();
        let address_bytes = &hash[12..]; // Last 20 bytes
        let signer_address = format!("0x{}", hex::encode(address_bytes));

        Ok(Self {
            secp,
            signing_key,
            signer_address,
        })
    }

    /// Get the signer's Ethereum address
    ///
    /// # Returns
    /// The 0x-prefixed Ethereum address derived from the signing key
    pub fn address(&self) -> &str {
        &self.signer_address
    }

    /// Sign a request body and return the X-Flashbots-Signature header value
    ///
    /// # Format
    /// Returns: "0xAddress:0xSignature"
    ///
    /// # Example
    /// ```rust
    /// let body = r#"{"jsonrpc":"2.0","method":"eth_sendBundle",...}"#;
    /// let signature = auth.sign_request(body)?;
    /// // Returns: "0x1234...abcd:0x5678...ef01"
    /// ```
    ///
    /// # Arguments
    /// * `body` - The complete JSON-RPC request body to sign
    ///
    /// # Returns
    /// * `Ok(String)` - The formatted signature header value
    /// * `Err(AuthError)` - If signing fails
    pub fn sign_request(&self, body: &str) -> Result<String, AuthError> {
        // EIP-191 personal message format
        let message_to_sign = self.create_eip191_message(body);

        // Hash the message
        let mut hasher = Keccak256::new();
        hasher.update(&message_to_sign);
        let hash = hasher.finalize();

        // Sign the hash
        let message =
            Message::from_slice(&hash).map_err(|e| AuthError::SigningError(e.to_string()))?;

        let signature = self
            .secp
            .sign_ecdsa_recoverable(&message, &self.signing_key);
        let (recovery_id, signature_bytes) = signature.serialize_compact();

        // Format signature as Ethereum does (v = recovery_id + 27)
        let mut eth_signature = [0u8; 65];
        eth_signature[..64].copy_from_slice(&signature_bytes);
        eth_signature[64] = recovery_id.to_i32() as u8 + 27;

        // Format as "0xAddress:0xSignature"
        Ok(format!(
            "{}:0x{}",
            self.signer_address,
            hex::encode(eth_signature)
        ))
    }

    /// Create an EIP-191 formatted message
    ///
    /// # Format
    /// "\x19Ethereum Signed Message:\n<length><message>"
    fn create_eip191_message(&self, body: &str) -> Vec<u8> {
        let prefix = format!("\x19Ethereum Signed Message:\n{}", body.len());
        let mut message = prefix.as_bytes().to_vec();
        message.extend_from_slice(body.as_bytes());
        message
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_authenticator_creation() {
        // Test key (DO NOT USE IN PRODUCTION)
        let key = "1111111111111111111111111111111111111111111111111111111111111111";
        let auth = FlashbotsAuthenticator::new(key).unwrap();

        // Verify address derivation
        assert!(auth.address().starts_with("0x"));
        assert_eq!(auth.address().len(), 42); // 0x + 40 hex chars
    }

    #[test]
    fn test_authenticator_with_0x_prefix() {
        // Test that 0x prefix is handled correctly
        let key = "0x1111111111111111111111111111111111111111111111111111111111111111";
        let auth = FlashbotsAuthenticator::new(key).unwrap();
        assert!(auth.address().starts_with("0x"));
    }

    #[test]
    fn test_signature_format() {
        let key = "1111111111111111111111111111111111111111111111111111111111111111";
        let auth = FlashbotsAuthenticator::new(key).unwrap();

        let body = r#"{"jsonrpc":"2.0","method":"eth_sendBundle","params":[],"id":1}"#;
        let signature = auth.sign_request(body).unwrap();

        // Verify format: address:signature
        let parts: Vec<&str> = signature.split(':').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], auth.address());
        assert!(parts[1].starts_with("0x"));
        assert_eq!(parts[1].len(), 132); // 0x + 65 bytes * 2 hex chars
    }

    #[test]
    fn test_invalid_key() {
        let result = FlashbotsAuthenticator::new("invalid-hex");
        assert!(result.is_err());

        let result = FlashbotsAuthenticator::new("00"); // Too short
        assert!(result.is_err());
    }

    #[test]
    fn test_deterministic_signatures() {
        let key = "1111111111111111111111111111111111111111111111111111111111111111";
        let auth = FlashbotsAuthenticator::new(key).unwrap();

        let body = r#"{"jsonrpc":"2.0","method":"eth_sendBundle","params":[],"id":1}"#;
        let sig1 = auth.sign_request(body).unwrap();
        let sig2 = auth.sign_request(body).unwrap();

        // Same input should produce same signature
        assert_eq!(sig1, sig2);
    }

    /// End-to-end EIP-191 round trip: sign a body, then recover the signer
    /// address from the signature alone. This is what Flashbots' relay does on
    /// the wire. A non-recoverable (64-byte) signature would fail this test —
    /// it caught the Path B implementation that used `sign_ecdsa` instead of
    /// `sign_ecdsa_recoverable` and silently authenticated as nobody.
    #[test]
    fn test_signature_recovers_to_signer_address() {
        use secp256k1::{
            ecdsa::{RecoverableSignature, RecoveryId},
            Message, Secp256k1,
        };

        let key = "1111111111111111111111111111111111111111111111111111111111111111";
        let auth = FlashbotsAuthenticator::new(key).unwrap();
        let body = r#"{"jsonrpc":"2.0","method":"eth_sendBundle","params":[],"id":1}"#;

        let header = auth.sign_request(body).unwrap();
        let (addr, sig_hex) = header.split_once(':').expect("address:signature header");
        let sig_bytes = hex::decode(sig_hex.trim_start_matches("0x")).unwrap();
        assert_eq!(
            sig_bytes.len(),
            65,
            "Flashbots requires 65-byte EIP-191 signature"
        );

        // Reconstruct the same EIP-191 hash the authenticator signed
        let prefix = format!("\x19Ethereum Signed Message:\n{}", body.len());
        let mut prefixed = prefix.into_bytes();
        prefixed.extend_from_slice(body.as_bytes());
        let hash = {
            let mut h = Keccak256::new();
            h.update(&prefixed);
            h.finalize()
        };
        let message = Message::from_slice(&hash).unwrap();

        // Decode v (last byte, +27 by Ethereum convention) into a RecoveryId
        let recovery_id = RecoveryId::from_i32(sig_bytes[64] as i32 - 27).unwrap();
        let recoverable =
            RecoverableSignature::from_compact(&sig_bytes[..64], recovery_id).unwrap();

        let secp = Secp256k1::new();
        let public_key = secp.recover_ecdsa(&message, &recoverable).unwrap();
        let pk_bytes = public_key.serialize_uncompressed();
        let mut h = Keccak256::new();
        h.update(&pk_bytes[1..]); // skip 0x04 prefix
        let pk_hash = h.finalize();
        let recovered = format!("0x{}", hex::encode(&pk_hash[12..]));

        assert_eq!(recovered, addr, "recovered address must match signer");
        assert_eq!(recovered, auth.address());
    }
}
