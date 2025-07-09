//! MEV (Maximum Extractable Value) protection types
//! 
//! This module defines the types used for MEV protection through Flashbots
//! and other MEV relay services.

use serde::{Deserialize, Serialize};

/// Bundle of transactions to be submitted atomically
/// 
/// A bundle represents a group of transactions that must be included together
/// in the same block. This is the core primitive for MEV protection.
/// 
/// # Fields
/// 
/// * `txs` - Array of signed transaction data (hex-encoded)
/// * `block_number` - Target block number for inclusion (hex-encoded)
/// * `min_timestamp` - Optional minimum timestamp for bundle validity
/// * `max_timestamp` - Optional maximum timestamp for bundle validity
/// 
/// # Example
/// 
/// ```json
/// {
///   "txs": ["0x...", "0x..."],
///   "blockNumber": "0x1234567",
///   "minTimestamp": 1234567890,
///   "maxTimestamp": 1234567900
/// }
/// ```
/// 
/// # Usage
/// 
/// This type is used by developers submitting transactions through the MEV-protected
/// endpoint. The bundle ensures that either all transactions are included together
/// or none are included, preventing sandwich attacks and other MEV extraction.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bundle {
    /// Array of signed transactions in the bundle
    pub txs: Vec<String>,
    
    /// Target block number for inclusion (hex-encoded)
    #[serde(rename = "blockNumber")]
    pub block_number: String,
    
    /// Optional minimum timestamp for bundle validity
    /// 
    /// If specified, the bundle will only be valid after this Unix timestamp.
    /// This is useful for time-locked transactions or ensuring proper ordering.
    #[serde(rename = "minTimestamp", skip_serializing_if = "Option::is_none")]
    pub min_timestamp: Option<u64>,
    
    /// Optional maximum timestamp for bundle validity
    /// 
    /// If specified, the bundle will only be valid before this Unix timestamp.
    /// This prevents stale bundles from being included in future blocks.
    #[serde(rename = "maxTimestamp", skip_serializing_if = "Option::is_none")]
    pub max_timestamp: Option<u64>,
}

/// Response from bundle submission
/// 
/// Contains the unique identifier for a submitted bundle, which can be used
/// to track its status or debug inclusion issues.
/// 
/// # Developer Usage
/// 
/// After submitting a bundle, developers receive this response containing
/// the bundle hash. This hash can be used to:
/// - Query bundle status
/// - Debug why a bundle wasn't included
/// - Correlate logs with specific submissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BundleResponse {
    /// Unique identifier for the submitted bundle
    #[serde(rename = "bundleHash")]
    pub bundle_hash: String,
}

/// Parameters for `eth_sendBundle` RPC method
/// 
/// This structure represents the parameters passed to the `eth_sendBundle`
/// method when submitting bundles through the MEV relay.
/// 
/// # Example Request
/// 
/// ```json
/// {
///   "jsonrpc": "2.0",
///   "method": "eth_sendBundle",
///   "params": [{
///     "txs": ["0x..."],
///     "blockNumber": "0x1234567"
///   }],
///   "id": 1
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendBundleParams {
    /// Array of signed transactions
    pub txs: Vec<String>,
    
    /// Target block number (hex)
    #[serde(rename = "blockNumber")]
    pub block_number: String,
    
    /// Optional minimum timestamp
    #[serde(rename = "minTimestamp", skip_serializing_if = "Option::is_none")]
    pub min_timestamp: Option<u64>,
    
    /// Optional maximum timestamp
    #[serde(rename = "maxTimestamp", skip_serializing_if = "Option::is_none")]
    pub max_timestamp: Option<u64>,
    
    /// Optional reverting transaction hashes
    /// 
    /// List of transaction hashes that are allowed to revert. This is useful
    /// when you want to include a transaction that might fail but shouldn't
    /// invalidate the entire bundle.
    #[serde(rename = "revertingTxHashes", skip_serializing_if = "Option::is_none")]
    pub reverting_tx_hashes: Option<Vec<String>>,
}

/// Error types specific to MEV operations
/// 
/// These errors help developers understand what went wrong during MEV
/// operations and how to fix their submissions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MevError {
    /// Error code (standard JSON-RPC or custom MEV codes)
    pub code: i32,
    
    /// Human-readable error message
    pub message: String,
    
    /// Optional additional error data
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<serde_json::Value>,
}

impl std::fmt::Display for MevError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MEV Error {}: {}", self.code, self.message)
    }
}

impl std::error::Error for MevError {}