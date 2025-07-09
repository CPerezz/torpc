use reqwest::Client;
use serde_json::json;
use anyhow::Result;

use crate::rpc_types::{JsonRpcRequest, JsonRpcResponse};

/// Simple Geth client for testing connectivity
pub struct GethClient {
    client: Client,
    url: String,
}

impl GethClient {
    pub fn new(url: String) -> Self {
        let client = Client::builder()
            .timeout(std::time::Duration::from_secs(10))
            .build()
            .expect("Failed to create HTTP client");
            
        Self { client, url }
    }
    
    /// Test basic connectivity by calling eth_blockNumber
    pub async fn test_connectivity(&self) -> Result<String> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "eth_blockNumber".to_string(),
            params: None,
            id: Some(json!(1)),
        };
        
        let response = self.client
            .post(&self.url)
            .json(&request)
            .send()
            .await?;
            
        if !response.status().is_success() {
            anyhow::bail!("Geth returned status: {}", response.status());
        }
        
        let json_response: JsonRpcResponse = response.json().await?;
        
        if let Some(error) = json_response.error {
            anyhow::bail!("RPC error: {}", error.message);
        }
        
        if let Some(result) = json_response.result {
            Ok(result.to_string())
        } else {
            anyhow::bail!("No result in response");
        }
    }
    
    /// Get the chain ID
    pub async fn get_chain_id(&self) -> Result<String> {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "eth_chainId".to_string(),
            params: None,
            id: Some(json!(1)),
        };
        
        let response = self.client
            .post(&self.url)
            .json(&request)
            .send()
            .await?;
            
        let json_response: JsonRpcResponse = response.json().await?;
        
        if let Some(result) = json_response.result {
            Ok(result.to_string())
        } else {
            anyhow::bail!("Failed to get chain ID");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use mockito::Server;
    
    #[tokio::test]
    async fn test_connectivity_success() {
        let mut server = Server::new_async().await;
        let _m = server.mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"jsonrpc":"2.0","result":"0x123","id":1}"#)
            .create();
            
        let client = GethClient::new(server.url());
        let result = client.test_connectivity().await.unwrap();
        assert_eq!(result, "\"0x123\"");
    }
    
    #[tokio::test]
    async fn test_connectivity_error() {
        let mut server = Server::new_async().await;
        let _m = server.mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"jsonrpc":"2.0","error":{"code":-32601,"message":"Method not found"},"id":1}"#)
            .create();
            
        let client = GethClient::new(server.url());
        let result = client.test_connectivity().await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Method not found"));
    }
    
    #[tokio::test]
    async fn test_get_chain_id() {
        let mut server = Server::new_async().await;
        let _m = server.mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"jsonrpc":"2.0","result":"0x539","id":1}"#)
            .create();
            
        let client = GethClient::new(server.url());
        let result = client.get_chain_id().await.unwrap();
        assert_eq!(result, "\"0x539\""); // 1337 in hex (dev chain ID)
    }
}

/// Standalone test binary
#[cfg(not(test))]
pub async fn test_geth_connection() -> Result<()> {
    use tracing::info;
    
    let url = std::env::var("GETH_URL")
        .unwrap_or_else(|_| "http://127.0.0.1:8545".to_string());
        
    info!("Testing Geth connection at: {}", url);
    
    let client = GethClient::new(url);
    
    // Test connectivity
    match client.test_connectivity().await {
        Ok(block_number) => {
            info!("✅ Geth is responding! Current block: {}", block_number);
        }
        Err(e) => {
            anyhow::bail!("❌ Failed to connect to Geth: {}", e);
        }
    }
    
    // Get chain ID
    match client.get_chain_id().await {
        Ok(chain_id) => {
            info!("Chain ID: {}", chain_id);
        }
        Err(e) => {
            anyhow::bail!("Failed to get chain ID: {}", e);
        }
    }
    
    Ok(())
}