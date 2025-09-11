use axum::{
    extract::Request,
    http::{HeaderValue, StatusCode},
    middleware::Next,
    response::Response,
};
use std::time::{Duration, Instant};
use tracing::{debug, warn, info};
use serde_json::json;
use tower::ServiceBuilder;
use tower_http::timeout::TimeoutLayer;

/// Security configuration
#[derive(Debug, Clone)]
pub struct SecurityConfig {
    pub max_body_size: usize,
    pub request_timeout: Duration,
    pub strict_headers: bool,
}

impl SecurityConfig {
    pub fn from_env() -> Self {
        let max_body_size = std::env::var("MAX_BODY_SIZE")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1024 * 1024); // 1MB default

        let request_timeout_secs = std::env::var("REQUEST_TIMEOUT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(30); // 30 seconds default

        let strict_headers = std::env::var("STRICT_HEADERS")
            .map(|s| s.to_lowercase() == "true")
            .unwrap_or(true);

        Self {
            max_body_size,
            request_timeout: Duration::from_secs(request_timeout_secs),
            strict_headers,
        }
    }
}

impl Default for SecurityConfig {
    fn default() -> Self {
        Self {
            max_body_size: 1024 * 1024, // 1MB
            request_timeout: Duration::from_secs(30),
            strict_headers: true,
        }
    }
}

/// Security metrics tracking
#[derive(Debug, Clone, Default)]
pub struct SecurityMetrics {
    pub blocked_requests_total: u64,
    pub rate_limit_hits: u64,
    pub oversized_requests: u64,
    pub invalid_methods: u64,
    pub suspicious_patterns: u64,
}

impl SecurityMetrics {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn increment_blocked_requests(&mut self) {
        self.blocked_requests_total += 1;
    }

    pub fn increment_rate_limit_hits(&mut self) {
        self.rate_limit_hits += 1;
    }

    pub fn increment_oversized_requests(&mut self) {
        self.oversized_requests += 1;
    }

    pub fn increment_invalid_methods(&mut self) {
        self.invalid_methods += 1;
    }

    pub fn increment_suspicious_patterns(&mut self) {
        self.suspicious_patterns += 1;
    }

    pub fn get_metrics_json(&self) -> serde_json::Value {
        json!({
            "blocked_requests_total": self.blocked_requests_total,
            "rate_limit_hits": self.rate_limit_hits,
            "oversized_requests": self.oversized_requests,
            "invalid_methods": self.invalid_methods,
            "suspicious_patterns": self.suspicious_patterns
        })
    }
}

/// Security event types for structured logging
#[derive(Debug, Clone)]
pub enum SecurityEventType {
    BlockedMethod,
    RateLimitExceeded,
    OversizedRequest,
    SuspiciousPattern,
    InvalidRequest,
}

/// Security event for logging
#[derive(Debug, Clone)]
pub struct SecurityEvent {
    pub event_type: SecurityEventType,
    pub method: Option<String>,
    pub size: Option<usize>,
    pub user_agent: Option<String>,
    pub timestamp: Instant,
    pub message: String,
}

impl SecurityEvent {
    pub fn new(event_type: SecurityEventType, message: String) -> Self {
        Self {
            event_type,
            method: None,
            size: None,
            user_agent: None,
            timestamp: Instant::now(),
            message,
        }
    }

    pub fn with_method(mut self, method: String) -> Self {
        self.method = Some(method);
        self
    }

    pub fn with_size(mut self, size: usize) -> Self {
        self.size = Some(size);
        self
    }

    pub fn with_user_agent(mut self, user_agent: String) -> Self {
        self.user_agent = Some(user_agent);
        self
    }

    pub fn log(&self) {
        let event_data = json!({
            "event_type": format!("{:?}", self.event_type),
            "method": self.method,
            "size": self.size,
            "user_agent": self.user_agent,
            "timestamp": format!("{:?}", self.timestamp.elapsed()),
            "message": self.message
        });

        match self.event_type {
            SecurityEventType::SuspiciousPattern => {
                warn!(security_event = %event_data, "Security event detected");
            },
            SecurityEventType::RateLimitExceeded => {
                info!(security_event = %event_data, "Rate limit exceeded");
            },
            _ => {
                debug!(security_event = %event_data, "Security event");
            }
        }
    }
}

/// Request pattern analyzer for detecting suspicious behavior
#[derive(Debug, Clone)]
pub struct RequestPatternAnalyzer {
    max_request_size: usize,
    known_methods: Vec<String>,
}

impl RequestPatternAnalyzer {
    pub fn new(max_request_size: usize) -> Self {
        let known_methods = vec![
            "eth_blockNumber".to_string(),
            "eth_getBalance".to_string(),
            "eth_getStorageAt".to_string(),
            "eth_getTransactionCount".to_string(),
            "eth_getBlockTransactionCountByHash".to_string(),
            "eth_getBlockTransactionCountByNumber".to_string(),
            "eth_getCode".to_string(),
            "eth_call".to_string(),
            "eth_estimateGas".to_string(),
            "eth_getBlockByHash".to_string(),
            "eth_getBlockByNumber".to_string(),
            "eth_getTransactionByHash".to_string(),
            "eth_getTransactionByBlockHashAndIndex".to_string(),
            "eth_getTransactionByBlockNumberAndIndex".to_string(),
            "eth_getTransactionReceipt".to_string(),
            "eth_getUncleByBlockHashAndIndex".to_string(),
            "eth_getUncleByBlockNumberAndIndex".to_string(),
            "eth_getUncleCountByBlockHash".to_string(),
            "eth_getUncleCountByBlockNumber".to_string(),
            "eth_protocolVersion".to_string(),
            "eth_chainId".to_string(),
            "eth_syncing".to_string(),
            "eth_gasPrice".to_string(),
            "eth_feeHistory".to_string(),
            "eth_maxPriorityFeePerGas".to_string(),
            "net_version".to_string(),
            "net_listening".to_string(),
            "net_peerCount".to_string(),
            "web3_clientVersion".to_string(),
            "web3_sha3".to_string(),
            "eth_sendRawTransaction".to_string(),
            "eth_sendBundle".to_string(),
            "eth_getLogs".to_string(),
        ];

        Self {
            max_request_size,
            known_methods,
        }
    }

    pub fn analyze_request(&self, method: &str, size: usize, user_agent: Option<&str>) -> Vec<SecurityEvent> {
        let mut events = Vec::new();

        // Check for oversized requests
        if size > self.max_request_size {
            let event = SecurityEvent::new(
                SecurityEventType::OversizedRequest,
                format!("Request size {} exceeds limit {}", size, self.max_request_size)
            )
            .with_method(method.to_string())
            .with_size(size);
            
            if let Some(ua) = user_agent {
                events.push(event.with_user_agent(ua.to_string()));
            } else {
                events.push(event);
            }
        }

        // Check for unknown methods
        if !self.known_methods.contains(&method.to_string()) {
            let event = SecurityEvent::new(
                SecurityEventType::SuspiciousPattern,
                format!("Unknown RPC method: {}", method)
            )
            .with_method(method.to_string());
            
            if let Some(ua) = user_agent {
                events.push(event.with_user_agent(ua.to_string()));
            } else {
                events.push(event);
            }
        }

        // Check for suspicious user agents (basic patterns)
        if let Some(ua) = user_agent {
            let suspicious_patterns = [
                "bot", "crawler", "spider", "scraper", "scanner",
                "nmap", "masscan", "zmap", "nuclei", "sqlmap"
            ];
            
            let ua_lower = ua.to_lowercase();
            for pattern in &suspicious_patterns {
                if ua_lower.contains(pattern) {
                    let event = SecurityEvent::new(
                        SecurityEventType::SuspiciousPattern,
                        format!("Suspicious user agent detected: {}", ua)
                    )
                    .with_method(method.to_string())
                    .with_user_agent(ua.to_string());
                    
                    events.push(event);
                    break;
                }
            }
        }

        events
    }
}

/// Add security headers to all responses
pub async fn add_security_headers(request: Request, next: Next) -> Response {
    let mut response = next.run(request).await;
    
    let headers = response.headers_mut();
    
    // Prevent MIME type sniffing
    headers.insert("X-Content-Type-Options", HeaderValue::from_static("nosniff"));
    
    // Prevent page from being displayed in a frame
    headers.insert("X-Frame-Options", HeaderValue::from_static("DENY"));
    
    // Disable legacy XSS protection (modern approach)
    headers.insert("X-XSS-Protection", HeaderValue::from_static("0"));
    
    // Control referrer information
    headers.insert("Referrer-Policy", HeaderValue::from_static("no-referrer"));
    
    // Content Security Policy - allows local resources while maintaining security
    headers.insert(
        "Content-Security-Policy", 
        HeaderValue::from_static("default-src 'self'; connect-src 'self' http://localhost:8081; style-src 'self' 'unsafe-inline'; script-src 'self'; img-src 'self' data:; frame-ancestors 'none'; base-uri 'self'; form-action 'self'")
    );
    
    // Prevent caching of responses
    headers.insert("Cache-Control", HeaderValue::from_static("no-store, no-cache, must-revalidate"));
    headers.insert("Pragma", HeaderValue::from_static("no-cache"));
    headers.insert("Expires", HeaderValue::from_static("0"));
    
    // Remove server header to prevent fingerprinting
    headers.remove("Server");
    
    // Add custom header to identify TorPC (optional)
    headers.insert("X-Service", HeaderValue::from_static("TorPC"));
    
    response
}

/// Request size and pattern monitoring middleware
pub async fn monitor_request_patterns(request: Request, next: Next) -> Response {
    let method = request.method().clone();
    let uri = request.uri().clone();
    let headers = request.headers().clone();
    let user_agent = headers.get("user-agent")
        .and_then(|h| h.to_str().ok())
        .map(|s| s.to_string());

    // Estimate request size (headers + body size if available)
    let content_length = headers.get("content-length")
        .and_then(|h| h.to_str().ok())
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(0);
    
    let estimated_size = headers.iter()
        .map(|(name, value)| name.as_str().len() + value.len())
        .sum::<usize>() + content_length;

    debug!(
        method = %method,
        uri = %uri,
        size = estimated_size,
        user_agent = user_agent.as_deref().unwrap_or("none"),
        "Request received"
    );

    // For JSON-RPC requests, we'll analyze the method in the proxy handlers
    // Here we just do basic size and header analysis
    let analyzer = RequestPatternAnalyzer::new(1024 * 1024); // 1MB limit

    // Basic pattern analysis
    if let Some(ref ua) = user_agent {
        let events = analyzer.analyze_request("unknown", estimated_size, Some(ua));
        for event in events {
            event.log();
        }
    }

    let response = next.run(request).await;

    debug!(
        method = %method,
        uri = %uri,
        status = %response.status(),
        "Request completed"
    );

    response
}

/// Health check endpoint that doesn't expose sensitive information
pub async fn health_check() -> Result<axum::Json<serde_json::Value>, StatusCode> {
    // Basic health indicators without sensitive data
    let health_data = json!({
        "status": "healthy",
        "service": "torpc",
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "version": env!("CARGO_PKG_VERSION"),
        // Basic connectivity check (could be expanded)
        "components": {
            "proxy": "ok",
            "handlers": "ok"
        }
    });

    Ok(axum::Json(health_data))
}

/// Simple metrics endpoint for security monitoring
pub async fn security_metrics() -> Result<axum::Json<serde_json::Value>, StatusCode> {
    // In a real implementation, these would be pulled from a shared state
    // For now, return a placeholder structure
    let metrics = SecurityMetrics::new();
    
    let metrics_data = json!({
        "security_metrics": metrics.get_metrics_json(),
        "timestamp": chrono::Utc::now().to_rfc3339(),
        "uptime": "placeholder", // Could track actual uptime
    });

    Ok(axum::Json(metrics_data))
}

/// Build security layers for the application
pub fn build_security_layers(config: SecurityConfig) -> ServiceBuilder<tower::layer::util::Stack<TimeoutLayer, tower::layer::util::Identity>> {
    ServiceBuilder::new()
        .layer(TimeoutLayer::new(config.request_timeout))
}

/// Security headers middleware (wrapper for add_security_headers)
pub async fn security_headers_middleware(request: Request, next: Next) -> Response {
    add_security_headers(request, next).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_pattern_analyzer() {
        let analyzer = RequestPatternAnalyzer::new(1024);

        // Test oversized request
        let events = analyzer.analyze_request("eth_blockNumber", 2048, None);
        assert!(!events.is_empty());
        assert!(matches!(events[0].event_type, SecurityEventType::OversizedRequest));

        // Test unknown method
        let events = analyzer.analyze_request("unknown_method", 512, None);
        assert!(!events.is_empty());
        assert!(matches!(events[0].event_type, SecurityEventType::SuspiciousPattern));

        // Test suspicious user agent
        let events = analyzer.analyze_request("eth_blockNumber", 512, Some("malicious-bot/1.0"));
        assert!(!events.is_empty());
        assert!(matches!(events[0].event_type, SecurityEventType::SuspiciousPattern));

        // Test normal request
        let events = analyzer.analyze_request("eth_blockNumber", 512, Some("Mozilla/5.0"));
        assert!(events.is_empty());
    }

    #[test]
    fn test_request_pattern_analyzer_edge_cases() {
        let analyzer = RequestPatternAnalyzer::new(1000);

        // Test exactly at size limit
        let events = analyzer.analyze_request("eth_blockNumber", 1000, None);
        assert!(events.is_empty());

        // Test one byte over limit
        let events = analyzer.analyze_request("eth_blockNumber", 1001, None);
        assert!(!events.is_empty());
        assert!(matches!(events[0].event_type, SecurityEventType::OversizedRequest));

        // Test multiple suspicious patterns (should only create one event)
        let events = analyzer.analyze_request("unknown_method", 512, Some("nmap-scanner"));
        assert_eq!(events.len(), 2); // One for unknown method, one for suspicious UA

        // Test all known methods are not flagged
        let known_methods = [
            "eth_blockNumber", "eth_getBalance", "eth_call", "eth_sendRawTransaction", 
            "eth_sendBundle", "net_version", "web3_clientVersion"
        ];
        
        for method in &known_methods {
            let events = analyzer.analyze_request(method, 512, Some("Mozilla/5.0"));
            assert!(events.is_empty(), "Method {} should not be flagged as suspicious", method);
        }
    }

    #[test]
    fn test_security_metrics() {
        let mut metrics = SecurityMetrics::new();
        
        metrics.increment_blocked_requests();
        metrics.increment_rate_limit_hits();
        
        assert_eq!(metrics.blocked_requests_total, 1);
        assert_eq!(metrics.rate_limit_hits, 1);
        
        let json = metrics.get_metrics_json();
        assert_eq!(json["blocked_requests_total"], 1);
        assert_eq!(json["rate_limit_hits"], 1);
    }

    #[test]
    fn test_security_metrics_all_increments() {
        let mut metrics = SecurityMetrics::new();
        
        // Test all increment methods
        metrics.increment_blocked_requests();
        metrics.increment_rate_limit_hits();
        metrics.increment_oversized_requests();
        metrics.increment_invalid_methods();
        metrics.increment_suspicious_patterns();
        
        assert_eq!(metrics.blocked_requests_total, 1);
        assert_eq!(metrics.rate_limit_hits, 1);
        assert_eq!(metrics.oversized_requests, 1);
        assert_eq!(metrics.invalid_methods, 1);
        assert_eq!(metrics.suspicious_patterns, 1);
        
        // Test JSON output includes all fields
        let json = metrics.get_metrics_json();
        assert_eq!(json["blocked_requests_total"], 1);
        assert_eq!(json["rate_limit_hits"], 1);
        assert_eq!(json["oversized_requests"], 1);
        assert_eq!(json["invalid_methods"], 1);
        assert_eq!(json["suspicious_patterns"], 1);
    }

    #[test]
    fn test_security_event() {
        let event = SecurityEvent::new(
            SecurityEventType::BlockedMethod,
            "Test message".to_string()
        )
        .with_method("test_method".to_string())
        .with_size(1024);

        assert!(matches!(event.event_type, SecurityEventType::BlockedMethod));
        assert_eq!(event.method, Some("test_method".to_string()));
        assert_eq!(event.size, Some(1024));
        assert_eq!(event.message, "Test message");
    }

    #[test]
    fn test_security_event_builder_pattern() {
        let event = SecurityEvent::new(
            SecurityEventType::SuspiciousPattern,
            "Suspicious activity detected".to_string()
        )
        .with_method("unknown_method".to_string())
        .with_size(2048)
        .with_user_agent("malicious-bot/1.0".to_string());

        assert_eq!(event.method, Some("unknown_method".to_string()));
        assert_eq!(event.size, Some(2048));
        assert_eq!(event.user_agent, Some("malicious-bot/1.0".to_string()));
        assert_eq!(event.message, "Suspicious activity detected");
    }

    #[test]
    fn test_security_config_defaults() {
        let config = SecurityConfig::default();
        assert_eq!(config.max_body_size, 1024 * 1024); // 1MB
        assert_eq!(config.request_timeout.as_secs(), 30);
        assert_eq!(config.strict_headers, true);
    }

    #[test]
    fn test_security_config_from_env() {
        // Test with no environment variables (should use defaults)
        let config = SecurityConfig::from_env();
        assert_eq!(config.max_body_size, 1024 * 1024);
        assert_eq!(config.request_timeout.as_secs(), 30);
        assert_eq!(config.strict_headers, true);

        // Test with environment variables set
        std::env::set_var("MAX_BODY_SIZE", "2097152"); // 2MB
        std::env::set_var("REQUEST_TIMEOUT", "60");
        std::env::set_var("STRICT_HEADERS", "false");
        
        let env_config = SecurityConfig::from_env();
        assert_eq!(env_config.max_body_size, 2097152);
        assert_eq!(env_config.request_timeout.as_secs(), 60);
        assert_eq!(env_config.strict_headers, false);
        
        // Test with invalid values (should fall back to defaults)
        std::env::set_var("MAX_BODY_SIZE", "invalid");
        std::env::set_var("REQUEST_TIMEOUT", "invalid");
        
        let fallback_config = SecurityConfig::from_env();
        assert_eq!(fallback_config.max_body_size, 1024 * 1024); // Default
        assert_eq!(fallback_config.request_timeout.as_secs(), 30); // Default
        
        // Clean up environment variables
        std::env::remove_var("MAX_BODY_SIZE");
        std::env::remove_var("REQUEST_TIMEOUT");
        std::env::remove_var("STRICT_HEADERS");
    }

    // Note: Direct testing of add_security_headers requires mocking Next 
    // which is complex. These are tested in integration tests instead.

    #[test]
    fn test_build_security_layers() {
        let config = SecurityConfig {
            max_body_size: 1024 * 1024,
            request_timeout: Duration::from_secs(30),
            strict_headers: true,
        };

        // Test that the function returns without panicking
        let _layers = build_security_layers(config);
        // The actual functionality is tested in integration tests
    }

    #[test]
    fn test_suspicious_user_agent_patterns() {
        let analyzer = RequestPatternAnalyzer::new(1024);
        
        let suspicious_agents = [
            "nmap-scanner", "masscan-probe", "nuclei/v1.0", "sqlmap/1.0",
            "some-bot-scanner", "web-crawler/2.0", "spider-tool", "scraper-v3"
        ];
        
        for agent in &suspicious_agents {
            let events = analyzer.analyze_request("eth_blockNumber", 500, Some(agent));
            assert!(!events.is_empty(), "User agent '{}' should be flagged as suspicious", agent);
            assert!(matches!(events[0].event_type, SecurityEventType::SuspiciousPattern));
        }
        
        let legitimate_agents = [
            "Mozilla/5.0 (Windows NT 10.0; Win64; x64) AppleWebKit/537.36",
            "curl/7.68.0",
            "PostmanRuntime/7.26.8",
            "axios/0.21.1",
            "okhttp/4.9.0"
        ];
        
        for agent in &legitimate_agents {
            let events = analyzer.analyze_request("eth_blockNumber", 500, Some(agent));
            // Should only be empty or contain non-suspicious events
            for event in &events {
                assert!(!matches!(event.event_type, SecurityEventType::SuspiciousPattern), 
                       "User agent '{}' should not be flagged as suspicious", agent);
            }
        }
    }
}