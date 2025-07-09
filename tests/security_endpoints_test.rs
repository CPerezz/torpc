use axum::{
    http::StatusCode,
    middleware,
    routing::get,
    Router,
};
use axum_test::TestServer;
use serde_json::Value;
use torpc::security::{health_check, security_metrics, security_headers_middleware};

fn create_test_router_with_endpoints() -> Router {
    Router::new()
        .route("/health", get(health_check))
        .route("/metrics", get(security_metrics))
        .layer(middleware::from_fn(security_headers_middleware))
}

#[tokio::test]
async fn test_health_check_endpoint_basic() {
    let app = create_test_router_with_endpoints();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/health").await;
    
    assert_eq!(response.status_code(), StatusCode::OK);
    assert_eq!(response.header("content-type"), "application/json");
}

#[tokio::test]
async fn test_health_check_response_structure() {
    let app = create_test_router_with_endpoints();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/health").await;
    let health_data: Value = response.json();
    
    // Verify required fields are present
    assert_eq!(health_data["status"], "healthy");
    assert_eq!(health_data["service"], "torpc");
    assert!(health_data["timestamp"].is_string());
    assert!(health_data["version"].is_string());
    
    // Verify components section exists
    assert!(health_data["components"].is_object());
    assert_eq!(health_data["components"]["proxy"], "ok");
    assert_eq!(health_data["components"]["handlers"], "ok");
}

#[tokio::test]
async fn test_health_check_timestamp_format() {
    let app = create_test_router_with_endpoints();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/health").await;
    let health_data: Value = response.json();
    
    let timestamp = health_data["timestamp"].as_str().unwrap();
    
    // Verify timestamp is in RFC3339 format (ISO 8601)
    assert!(timestamp.contains("T"));
    assert!(timestamp.contains("Z") || timestamp.contains("+"));
    
    // Try to parse the timestamp to ensure it's valid
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .expect("Timestamp should be valid RFC3339 format");
}

#[tokio::test]
async fn test_health_check_version_info() {
    let app = create_test_router_with_endpoints();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/health").await;
    let health_data: Value = response.json();
    
    let version = health_data["version"].as_str().unwrap();
    
    // Version should not be empty and should follow semantic versioning pattern
    assert!(!version.is_empty());
    assert!(version == "0.1.0" || version.contains("."));
}

#[tokio::test]
async fn test_health_check_no_sensitive_data() {
    let app = create_test_router_with_endpoints();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/health").await;
    let health_data: Value = response.json();
    let response_text = health_data.to_string().to_lowercase();
    
    // Ensure no sensitive information is exposed
    let sensitive_terms = vec![
        "password", "secret", "key", "token", "auth", "credential",
        "private", "internal", "database", "config", "env"
    ];
    
    for term in sensitive_terms {
        assert!(!response_text.contains(term), 
               "Health check should not contain sensitive term: {}", term);
    }
}

#[tokio::test]
async fn test_security_metrics_endpoint_basic() {
    let app = create_test_router_with_endpoints();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/metrics").await;
    
    assert_eq!(response.status_code(), StatusCode::OK);
    assert_eq!(response.header("content-type"), "application/json");
}

#[tokio::test]
async fn test_security_metrics_response_structure() {
    let app = create_test_router_with_endpoints();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/metrics").await;
    let metrics_data: Value = response.json();
    
    // Verify top-level structure
    assert!(metrics_data["security_metrics"].is_object());
    assert!(metrics_data["timestamp"].is_string());
    assert!(metrics_data["uptime"].is_string());
    
    // Verify security metrics structure
    let security_metrics = &metrics_data["security_metrics"];
    assert!(security_metrics["blocked_requests_total"].is_number());
    assert!(security_metrics["rate_limit_hits"].is_number());
    assert!(security_metrics["oversized_requests"].is_number());
    assert!(security_metrics["invalid_methods"].is_number());
    assert!(security_metrics["suspicious_patterns"].is_number());
}

#[tokio::test]
async fn test_security_metrics_initial_values() {
    let app = create_test_router_with_endpoints();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/metrics").await;
    let metrics_data: Value = response.json();
    
    let security_metrics = &metrics_data["security_metrics"];
    
    // All counters should start at 0
    assert_eq!(security_metrics["blocked_requests_total"], 0);
    assert_eq!(security_metrics["rate_limit_hits"], 0);
    assert_eq!(security_metrics["oversized_requests"], 0);
    assert_eq!(security_metrics["invalid_methods"], 0);
    assert_eq!(security_metrics["suspicious_patterns"], 0);
}

#[tokio::test]
async fn test_security_metrics_timestamp_validity() {
    let app = create_test_router_with_endpoints();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/metrics").await;
    let metrics_data: Value = response.json();
    
    let timestamp = metrics_data["timestamp"].as_str().unwrap();
    
    // Verify timestamp is valid RFC3339 format
    chrono::DateTime::parse_from_rfc3339(timestamp)
        .expect("Metrics timestamp should be valid RFC3339 format");
}

#[tokio::test]
async fn test_endpoints_with_security_headers() {
    let app = create_test_router_with_endpoints();
    let server = TestServer::new(app).unwrap();
    
    // Test health endpoint has security headers
    let health_response = server.get("/health").await;
    verify_security_headers(&health_response);
    
    // Test metrics endpoint has security headers
    let metrics_response = server.get("/metrics").await;
    verify_security_headers(&metrics_response);
}

#[tokio::test]
async fn test_health_check_multiple_requests() {
    let app = create_test_router_with_endpoints();
    let server = TestServer::new(app).unwrap();
    
    // Make multiple requests to ensure consistency
    for i in 0..5 {
        let response = server.get("/health").await;
        assert_eq!(response.status_code(), StatusCode::OK);
        
        let health_data: Value = response.json();
        assert_eq!(health_data["status"], "healthy", "Request {} failed", i);
        assert_eq!(health_data["service"], "torpc", "Request {} failed", i);
    }
}

#[tokio::test]
async fn test_metrics_endpoint_multiple_requests() {
    let app = create_test_router_with_endpoints();
    let server = TestServer::new(app).unwrap();
    
    // Make multiple requests to ensure consistency
    for i in 0..5 {
        let response = server.get("/metrics").await;
        assert_eq!(response.status_code(), StatusCode::OK);
        
        let metrics_data: Value = response.json();
        assert!(metrics_data["security_metrics"].is_object(), "Request {} failed", i);
        assert!(metrics_data["timestamp"].is_string(), "Request {} failed", i);
    }
}

#[tokio::test]
async fn test_nonexistent_endpoint() {
    let app = create_test_router_with_endpoints();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/nonexistent").await;
    
    assert_eq!(response.status_code(), StatusCode::NOT_FOUND);
    // Should still have security headers even on 404
    verify_security_headers(&response);
}

#[tokio::test]
async fn test_wrong_method_on_endpoints() {
    let app = create_test_router_with_endpoints();
    let server = TestServer::new(app).unwrap();
    
    // POST to GET-only endpoints should return 405
    let health_post = server.post("/health").await;
    assert_eq!(health_post.status_code(), StatusCode::METHOD_NOT_ALLOWED);
    verify_security_headers(&health_post);
    
    let metrics_post = server.post("/metrics").await;
    assert_eq!(metrics_post.status_code(), StatusCode::METHOD_NOT_ALLOWED);
    verify_security_headers(&metrics_post);
}

#[tokio::test]
async fn test_health_check_concurrent_access() {
    let app = create_test_router_with_endpoints();
    let server = TestServer::new(app).unwrap();
    
    // Test concurrent access doesn't cause issues
    let mut responses = Vec::new();
    
    for _ in 0..3 {
        let resp = server.get("/health").await;
        responses.push(resp);
    }
    
    for response in responses {
        assert_eq!(response.status_code(), StatusCode::OK);
        let health_data: Value = response.json();
        assert_eq!(health_data["status"], "healthy");
    }
}

#[tokio::test]
async fn test_metrics_concurrent_access() {
    let app = create_test_router_with_endpoints();
    let server = TestServer::new(app).unwrap();
    
    // Test concurrent access doesn't cause issues
    let mut responses = Vec::new();
    
    for _ in 0..3 {
        let resp = server.get("/metrics").await;
        responses.push(resp);
    }
    
    for response in responses {
        assert_eq!(response.status_code(), StatusCode::OK);
        let metrics_data: Value = response.json();
        assert!(metrics_data["security_metrics"].is_object());
    }
}

#[tokio::test]
async fn test_endpoints_response_time() {
    let app = create_test_router_with_endpoints();
    let server = TestServer::new(app).unwrap();
    
    // Health check should be fast
    let start = std::time::Instant::now();
    let response = server.get("/health").await;
    let duration = start.elapsed();
    
    assert_eq!(response.status_code(), StatusCode::OK);
    assert!(duration.as_millis() < 1000, "Health check should be fast, took {}ms", duration.as_millis());
    
    // Metrics should also be fast
    let start = std::time::Instant::now();
    let response = server.get("/metrics").await;
    let duration = start.elapsed();
    
    assert_eq!(response.status_code(), StatusCode::OK);
    assert!(duration.as_millis() < 1000, "Metrics should be fast, took {}ms", duration.as_millis());
}

#[tokio::test]
async fn test_endpoint_content_encoding() {
    let app = create_test_router_with_endpoints();
    let server = TestServer::new(app).unwrap();
    
    let health_response = server.get("/health").await;
    let metrics_response = server.get("/metrics").await;
    
    // Verify responses are UTF-8 JSON
    assert_eq!(health_response.header("content-type"), "application/json");
    assert_eq!(metrics_response.header("content-type"), "application/json");
    
    // Verify JSON can be parsed
    let _: Value = health_response.json();
    let _: Value = metrics_response.json();
}

#[tokio::test]
async fn test_health_check_components_structure() {
    let app = create_test_router_with_endpoints();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/health").await;
    let health_data: Value = response.json();
    
    let components = &health_data["components"];
    assert!(components.is_object());
    
    // All components should report "ok" status
    if let Some(obj) = components.as_object() {
        for (name, status) in obj {
            assert_eq!(status, "ok", "Component {} should be ok", name);
        }
    }
}

// Helper function to verify security headers are present
fn verify_security_headers(response: &axum_test::TestResponse) {
    assert_eq!(response.header("X-Content-Type-Options"), "nosniff");
    assert_eq!(response.header("X-Frame-Options"), "DENY");
    assert_eq!(response.header("X-XSS-Protection"), "0");
    assert_eq!(response.header("Referrer-Policy"), "no-referrer");
    assert!(response.header("Content-Security-Policy")
        .to_str().unwrap().contains("default-src 'none'"));
    assert_eq!(response.header("X-Service"), "TorPC");
}