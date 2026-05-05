//! MEV-specific types for Flashbots bundle submission
//! 
//! This module contains all the data structures used for MEV protection
//! and bundle submission to Flashbots relays.

use serde::{Deserialize, Serialize};
use std::time::Instant;

/// Represents a bundle of transactions to be submitted to Flashbots
/// 
/// A bundle is a collection of transactions that must be executed atomically
/// in the exact order specified. Bundles are the core primitive of Flashbots
/// and enable MEV protection and extraction strategies.
/// 
/// # For Developers
/// This struct is used internally by the MEV client to format transaction
/// submissions according to Flashbots specifications.
/// 
/// # Example
/// ```rust
/// let bundle = Bundle {
///     txs: vec!["0xabc123...".to_string()], // Signed transaction
///     block_number: "0x1234567".to_string(), // Target block in hex
///     min_timestamp: Some(1625097600),       // Not before July 1, 2021
///     max_timestamp: Some(1625097900),       // Not after 5 minutes later
/// };
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    /// List of signed transactions in hex format (0x-prefixed)
    /// Order matters - transactions execute in array order
    pub txs: Vec<String>,
    
    /// Target block number for inclusion (hex format)
    /// Bundle will only be considered for this specific block
    #[serde(rename = "blockNumber")]
    pub block_number: String,
    
    /// Optional: Minimum Unix timestamp for bundle inclusion
    /// Bundle won't be included before this time
    /// Use case: Time-sensitive arbitrage, auction participation
    #[serde(rename = "minTimestamp", skip_serializing_if = "Option::is_none")]
    pub min_timestamp: Option<u64>,
    
    /// Optional: Maximum Unix timestamp for bundle inclusion  
    /// Bundle won't be included after this time
    /// Use case: Prevent stale trades, deadline enforcement
    #[serde(rename = "maxTimestamp", skip_serializing_if = "Option::is_none")]
    pub max_timestamp: Option<u64>,
}

/// Request format for eth_sendBundle JSON-RPC method
/// 
/// # For Library Developers
/// This wraps the bundle with additional metadata required by the
/// Flashbots relay API specification.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendBundleRequest {
    /// JSON-RPC version (always "2.0")
    pub jsonrpc: String,
    
    /// Method name (always "eth_sendBundle")
    pub method: String,
    
    /// Parameters array containing the bundle
    pub params: Vec<Bundle>,
    
    /// Request ID for correlation
    pub id: u64,
}

impl SendBundleRequest {
    /// Create a new bundle submission request
    pub fn new(bundle: Bundle, id: u64) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            method: "eth_sendBundle".to_string(),
            params: vec![bundle],
            id,
        }
    }
}

/// Response from Flashbots relay after bundle submission
/// 
/// # For Developers
/// Parse this response to determine if bundle was accepted and
/// monitor simulation results for debugging failed bundles.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleResponse {
    /// Unique identifier for the submitted bundle
    /// Use this to track bundle status
    #[serde(rename = "bundleHash")]
    pub bundle_hash: String,
    
    /// Optional simulation results if relay simulated the bundle
    #[serde(skip_serializing_if = "Option::is_none")]
    pub simulation: Option<SimulationResult>,
}

/// Results from bundle simulation
/// 
/// # For Developers
/// When a bundle fails simulation, these results help identify
/// the issue (e.g., insufficient gas, reverted transaction).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimulationResult {
    /// Whether all transactions in the bundle succeeded
    pub success: bool,
    
    /// Error message if simulation failed
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    
    /// Total gas used by all transactions
    #[serde(rename = "totalGasUsed", skip_serializing_if = "Option::is_none")]
    pub gas_used: Option<u64>,
    
    /// Coinbase payment if bundle includes miner tips
    #[serde(rename = "coinbaseDiff", skip_serializing_if = "Option::is_none")]
    pub coinbase_diff: Option<String>,
}

/// Circuit breaker states for managing relay failures.
///
/// The failure counter lives in `Closed`, not `Open`, because the only role
/// of the counter is to *decide when to open*. Once Open, the relevant state
/// is "when we opened" so we can transition to HalfOpen after the recovery
/// timeout. Holding both fields in the same variant previously produced a
/// state machine that lost its counter every time it failed to cross the
/// threshold (see git history for the original bug).
///
/// # State Transitions
/// ```text
/// Closed{n}    --(failure, n+1 >= threshold)--> Open{now}
/// Closed{n}    --(failure, n+1 <  threshold)--> Closed{n+1}
/// Closed{_}    --(success)-------------------->  Closed{0}
/// Open{at}     --(elapsed >= recovery_timeout)-> HalfOpen   (lazy, on next can_proceed)
/// HalfOpen     --(success)----------------------> Closed{0}
/// HalfOpen     --(failure)----------------------> Open{now}
/// ```
#[derive(Debug, Clone, PartialEq)]
pub enum CircuitState {
    /// Normal operation — requests pass through.
    /// `consecutive_failures` is the running count toward the open threshold.
    Closed { consecutive_failures: u32 },

    /// Relay is down — requests fail immediately until `recovery_timeout`
    /// elapses, after which the next `can_proceed` flips us to `HalfOpen`.
    Open { opened_at: Instant },

    /// Testing if relay has recovered — allow one request through; success
    /// closes the circuit, failure reopens it.
    HalfOpen,
}

/// Configuration for MEV relay client
/// 
/// # For Proxy Operators
/// Configure MEV protection behavior through environment variables
/// or configuration files.
#[derive(Debug, Clone)]
pub struct MevConfig {
    /// Relay endpoint URL (e.g., "https://relay.flashbots.net")
    pub relay_url: String,
    
    /// Private key for signing authentication headers (hex format)
    pub signing_key: String,
    
    /// Maximum time to wait for relay response
    pub request_timeout: std::time::Duration,
    
    /// Number of blocks ahead to target for bundle inclusion
    /// Default: 1 (next block)
    pub blocks_ahead: u64,
}

impl Default for MevConfig {
    fn default() -> Self {
        Self {
            relay_url: "https://relay.flashbots.net".to_string(),
            signing_key: String::new(),
            request_timeout: std::time::Duration::from_secs(5),
            blocks_ahead: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json;

    #[test]
    fn test_bundle_serialization() {
        let bundle = Bundle {
            txs: vec!["0xabc123".to_string()],
            block_number: "0x1234".to_string(),
            min_timestamp: Some(1625097600),
            max_timestamp: None,
        };
        
        let json = serde_json::to_string(&bundle).unwrap();
        assert!(json.contains("\"blockNumber\":\"0x1234\""));
        assert!(json.contains("\"minTimestamp\":1625097600"));
        assert!(!json.contains("maxTimestamp")); // Should be omitted when None
    }
    
    #[test]
    fn test_send_bundle_request() {
        let bundle = Bundle {
            txs: vec!["0xabc123".to_string()],
            block_number: "0x1234".to_string(),
            min_timestamp: None,
            max_timestamp: None,
        };
        
        let request = SendBundleRequest::new(bundle, 42);
        assert_eq!(request.method, "eth_sendBundle");
        assert_eq!(request.id, 42);
        assert_eq!(request.params.len(), 1);
    }
}