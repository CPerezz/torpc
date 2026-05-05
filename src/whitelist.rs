use once_cell::sync::Lazy;
use std::collections::HashSet;

/// List of allowed RPC methods
static ALLOWED_METHODS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    let mut methods = HashSet::new();

    // Read-only methods
    methods.insert("eth_blockNumber");
    methods.insert("eth_getBalance");
    methods.insert("eth_getStorageAt");
    methods.insert("eth_getTransactionCount");
    methods.insert("eth_getBlockTransactionCountByHash");
    methods.insert("eth_getBlockTransactionCountByNumber");
    methods.insert("eth_getCode");
    methods.insert("eth_call");
    methods.insert("eth_estimateGas");
    methods.insert("eth_getBlockByHash");
    methods.insert("eth_getBlockByNumber");
    methods.insert("eth_getTransactionByHash");
    methods.insert("eth_getTransactionByBlockHashAndIndex");
    methods.insert("eth_getTransactionByBlockNumberAndIndex");
    methods.insert("eth_getTransactionReceipt");
    methods.insert("eth_getUncleByBlockHashAndIndex");
    methods.insert("eth_getUncleByBlockNumberAndIndex");
    methods.insert("eth_getUncleCountByBlockHash");
    methods.insert("eth_getUncleCountByBlockNumber");
    methods.insert("eth_protocolVersion");
    methods.insert("eth_chainId");
    methods.insert("eth_syncing");
    methods.insert("eth_gasPrice");
    methods.insert("eth_feeHistory");
    methods.insert("eth_maxPriorityFeePerGas");
    methods.insert("eth_getLogs");

    // Network info
    methods.insert("net_version");
    methods.insert("net_listening");
    methods.insert("net_peerCount");

    // Web3 methods
    methods.insert("web3_clientVersion");
    methods.insert("web3_sha3");

    // Write methods we allow
    methods.insert("eth_sendRawTransaction");
    methods.insert("eth_sendBundle"); // Flashbots bundle submission

    methods
});

/// Check if a method is allowed
pub fn is_method_allowed(method: &str) -> bool {
    ALLOWED_METHODS.contains(method)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_allowed_read_methods() {
        // Test common read methods
        assert!(is_method_allowed("eth_blockNumber"));
        assert!(is_method_allowed("eth_getBalance"));
        assert!(is_method_allowed("eth_call"));
        assert!(is_method_allowed("eth_getTransactionReceipt"));
        assert!(is_method_allowed("net_version"));
        assert!(is_method_allowed("web3_clientVersion"));
    }

    #[test]
    fn test_allowed_write_method() {
        // Only eth_sendRawTransaction should be allowed
        assert!(is_method_allowed("eth_sendRawTransaction"));
    }

    #[test]
    fn test_blocked_dangerous_methods() {
        // These methods should be blocked
        assert!(!is_method_allowed("eth_sendTransaction"));
        assert!(!is_method_allowed("eth_sign"));
        assert!(!is_method_allowed("eth_signTransaction"));
        assert!(!is_method_allowed("personal_sign"));
        assert!(!is_method_allowed("personal_unlockAccount"));
        assert!(!is_method_allowed("eth_accounts"));
        assert!(!is_method_allowed("eth_coinbase"));
        assert!(!is_method_allowed("eth_mining"));
        assert!(!is_method_allowed("miner_start"));
        assert!(!is_method_allowed("miner_stop"));
        assert!(!is_method_allowed("admin_addPeer"));
        assert!(!is_method_allowed("debug_traceTransaction"));
    }

    #[test]
    fn test_case_sensitivity() {
        // Methods should be case-sensitive
        assert!(is_method_allowed("eth_blockNumber"));
        assert!(!is_method_allowed("ETH_BLOCKNUMBER"));
        assert!(!is_method_allowed("Eth_BlockNumber"));
    }

    #[test]
    fn test_no_empty_method() {
        assert!(!is_method_allowed(""));
        assert!(!is_method_allowed(" "));
    }
}
