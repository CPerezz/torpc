use axum::{
    http::StatusCode,
    response::{IntoResponse, Response},
    Json,
};
use thiserror::Error;

use crate::rpc_types::JsonRpcResponse;

#[derive(Error, Debug, Clone)]
pub enum ProxyError {
    #[error("Invalid JSON-RPC request: {0}")]
    InvalidRequest(String),
    
    #[error("Method not allowed: {0}")]
    MethodNotAllowed(String),
    
    #[error("Rate limit exceeded")]
    RateLimitExceeded,
    
    #[error("Upstream connection error: {0}")]
    UpstreamError(String),
    
    #[error("JSON parsing error: {0}")]
    JsonError(String),
    
    #[error("HTTP request error: {0}")]
    HttpError(String),
    
    #[error("Internal server error: {0}")]
    InternalError(String),
    
    #[error("MEV relay error: {0}")]
    MevRelayError(String),
    
    #[error("Bundle simulation failed: {0}")]
    BundleSimulationError(String),
    
    #[error("Flashbots authentication failed")]
    FlashbotsAuthError,
}

impl ProxyError {
    /// Convert to JSON-RPC error code
    pub fn to_json_rpc_code(&self) -> i64 {
        match self {
            ProxyError::InvalidRequest(_) => -32600, // Invalid Request
            ProxyError::MethodNotAllowed(_) => -32601, // Method not found
            ProxyError::RateLimitExceeded => -32000, // Server error
            ProxyError::JsonError(_) => -32700, // Parse error
            ProxyError::UpstreamError(_) => -32001, // Server error
            ProxyError::HttpError(_) => -32002, // Server error
            ProxyError::InternalError(_) => -32603, // Internal error
            ProxyError::MevRelayError(_) => -32003, // MEV relay error
            ProxyError::BundleSimulationError(_) => -32004, // Bundle simulation error
            ProxyError::FlashbotsAuthError => -32005, // Authentication error
        }
    }
    
    /// Convert to JSON-RPC error response
    pub fn to_json_rpc_response(&self, id: Option<serde_json::Value>) -> JsonRpcResponse {
        JsonRpcResponse::error(
            id,
            self.to_json_rpc_code(),
            self.to_string(),
            None,
        )
    }
}

impl IntoResponse for ProxyError {
    fn into_response(self) -> Response {
        let status_code = match &self {
            ProxyError::InvalidRequest(_) => StatusCode::BAD_REQUEST,
            ProxyError::MethodNotAllowed(_) => StatusCode::METHOD_NOT_ALLOWED,
            ProxyError::RateLimitExceeded => StatusCode::TOO_MANY_REQUESTS,
            ProxyError::JsonError(_) => StatusCode::BAD_REQUEST,
            _ => StatusCode::INTERNAL_SERVER_ERROR,
        };
        
        let error_response = self.to_json_rpc_response(None);
        
        (status_code, Json(error_response)).into_response()
    }
}

pub type ProxyResult<T> = Result<T, ProxyError>;

impl From<serde_json::Error> for ProxyError {
    fn from(err: serde_json::Error) -> Self {
        ProxyError::JsonError(err.to_string())
    }
}

impl From<reqwest::Error> for ProxyError {
    fn from(err: reqwest::Error) -> Self {
        ProxyError::HttpError(err.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_error_codes() {
        assert_eq!(
            ProxyError::InvalidRequest("test".to_string()).to_json_rpc_code(),
            -32600
        );
        assert_eq!(
            ProxyError::MethodNotAllowed("eth_accounts".to_string()).to_json_rpc_code(),
            -32601
        );
        assert_eq!(
            ProxyError::RateLimitExceeded.to_json_rpc_code(),
            -32000
        );
        assert_eq!(
            ProxyError::JsonError("Parse error".to_string()).to_json_rpc_code(),
            -32700
        );
    }

    #[test]
    fn test_error_to_json_rpc_response() {
        let error = ProxyError::MethodNotAllowed("eth_accounts".to_string());
        let response = error.to_json_rpc_response(Some(json!(1)));
        
        assert_eq!(response.jsonrpc, "2.0");
        assert!(response.result.is_none());
        assert!(response.error.is_some());
        
        let rpc_error = response.error.unwrap();
        assert_eq!(rpc_error.code, -32601);
        assert_eq!(rpc_error.message, "Method not allowed: eth_accounts");
    }

    #[test]
    fn test_error_messages() {
        let invalid_request = ProxyError::InvalidRequest("missing jsonrpc".to_string());
        assert_eq!(invalid_request.to_string(), "Invalid JSON-RPC request: missing jsonrpc");
        
        let method_not_allowed = ProxyError::MethodNotAllowed("eth_sign".to_string());
        assert_eq!(method_not_allowed.to_string(), "Method not allowed: eth_sign");
        
        let rate_limit = ProxyError::RateLimitExceeded;
        assert_eq!(rate_limit.to_string(), "Rate limit exceeded");
    }
}