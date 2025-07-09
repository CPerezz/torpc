use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 Request
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    pub params: Option<Value>,
    pub id: Option<Value>,
}

/// JSON-RPC 2.0 Response
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
    pub id: Option<Value>,
}

/// JSON-RPC 2.0 Error
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcRequest {
    /// Validate that this is a valid JSON-RPC 2.0 request
    pub fn validate(&self) -> Result<(), &'static str> {
        if self.jsonrpc != "2.0" {
            return Err("Invalid jsonrpc version");
        }
        if self.method.is_empty() {
            return Err("Method cannot be empty");
        }
        Ok(())
    }
}

impl JsonRpcResponse {
    /// Create a successful response
    pub fn success(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: Some(result),
            error: None,
            id,
        }
    }

    /// Create an error response
    pub fn error(id: Option<Value>, code: i64, message: String, data: Option<Value>) -> Self {
        Self {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(JsonRpcError {
                code,
                message,
                data,
            }),
            id,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_request_deserialization() {
        let json = r#"{
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 1
        }"#;

        let request: JsonRpcRequest = serde_json::from_str(json).unwrap();
        assert_eq!(request.jsonrpc, "2.0");
        assert_eq!(request.method, "eth_blockNumber");
        assert_eq!(request.id, Some(json!(1)));
    }

    #[test]
    fn test_request_validation() {
        let valid_request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "eth_blockNumber".to_string(),
            params: None,
            id: Some(json!(1)),
        };
        assert!(valid_request.validate().is_ok());

        let invalid_version = JsonRpcRequest {
            jsonrpc: "1.0".to_string(),
            method: "eth_blockNumber".to_string(),
            params: None,
            id: Some(json!(1)),
        };
        assert_eq!(invalid_version.validate(), Err("Invalid jsonrpc version"));

        let empty_method = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "".to_string(),
            params: None,
            id: Some(json!(1)),
        };
        assert_eq!(empty_method.validate(), Err("Method cannot be empty"));
    }

    #[test]
    fn test_response_creation() {
        let success_response = JsonRpcResponse::success(Some(json!(1)), json!("0x123"));
        assert_eq!(success_response.jsonrpc, "2.0");
        assert!(success_response.result.is_some());
        assert!(success_response.error.is_none());

        let error_response =
            JsonRpcResponse::error(Some(json!(1)), -32601, "Method not found".to_string(), None);
        assert_eq!(error_response.jsonrpc, "2.0");
        assert!(error_response.result.is_none());
        assert!(error_response.error.is_some());
    }

    #[test]
    fn test_serialization_roundtrip() {
        let request = JsonRpcRequest {
            jsonrpc: "2.0".to_string(),
            method: "eth_getBalance".to_string(),
            params: Some(json!(["0x123", "latest"])),
            id: Some(json!(42)),
        };

        let json = serde_json::to_string(&request).unwrap();
        let deserialized: JsonRpcRequest = serde_json::from_str(&json).unwrap();

        assert_eq!(request.method, deserialized.method);
        assert_eq!(request.params, deserialized.params);
        assert_eq!(request.id, deserialized.id);
    }
}
