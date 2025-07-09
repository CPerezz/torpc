use axum::{
    http::{HeaderName, HeaderValue, StatusCode},
    middleware,
    routing::{get, post},
    Router,
};
use axum_test::TestServer;
use serde_json::json;
use torpc::security::security_headers_middleware;

// Simple test handler that returns a basic response
async fn test_handler() -> &'static str {
    "test response"
}

// Test handler that returns JSON
async fn json_handler() -> axum::Json<serde_json::Value> {
    axum::Json(json!({"status": "ok", "data": "test"}))
}

// Test handler that returns an error
async fn error_handler() -> Result<&'static str, StatusCode> {
    Err(StatusCode::INTERNAL_SERVER_ERROR)
}

fn create_test_router() -> Router {
    Router::new()
        .route("/test", get(test_handler))
        .route("/json", get(json_handler))
        .route("/error", get(error_handler))
        .route("/post", post(test_handler))
        .layer(middleware::from_fn(security_headers_middleware))
}

#[tokio::test]
async fn test_security_headers_on_successful_get_request() {
    let app = create_test_router();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/test").await;
    
    assert_eq!(response.status_code(), StatusCode::OK);
    verify_security_headers(&response);
}

#[tokio::test]
async fn test_security_headers_on_successful_post_request() {
    let app = create_test_router();
    let server = TestServer::new(app).unwrap();
    
    let response = server.post("/post").await;
    
    assert_eq!(response.status_code(), StatusCode::OK);
    verify_security_headers(&response);
}

#[tokio::test]
async fn test_security_headers_on_json_response() {
    let app = create_test_router();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/json").await;
    
    assert_eq!(response.status_code(), StatusCode::OK);
    verify_security_headers(&response);
    
    // Verify JSON response is still valid
    let json_response: serde_json::Value = response.json();
    assert_eq!(json_response["status"], "ok");
}

#[tokio::test]
async fn test_security_headers_on_error_response() {
    let app = create_test_router();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/error").await;
    
    assert_eq!(response.status_code(), StatusCode::INTERNAL_SERVER_ERROR);
    verify_security_headers(&response);
}

#[tokio::test]
async fn test_security_headers_on_not_found() {
    let app = create_test_router();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/nonexistent").await;
    
    assert_eq!(response.status_code(), StatusCode::NOT_FOUND);
    verify_security_headers(&response);
}

#[tokio::test]
async fn test_security_headers_on_method_not_allowed() {
    let app = create_test_router();
    let server = TestServer::new(app).unwrap();
    
    // Try to POST to a GET-only endpoint
    let response = server.post("/test").await;
    
    assert_eq!(response.status_code(), StatusCode::METHOD_NOT_ALLOWED);
    verify_security_headers(&response);
}

#[tokio::test]
async fn test_security_headers_with_user_agent() {
    let app = create_test_router();
    let server = TestServer::new(app).unwrap();
    
    let response = server
        .get("/test")
        .add_header(
            HeaderName::from_static("user-agent"),
            HeaderValue::from_static("Mozilla/5.0")
        )
        .await;
    
    assert_eq!(response.status_code(), StatusCode::OK);
    verify_security_headers(&response);
}

#[tokio::test]
async fn test_security_headers_with_custom_headers() {
    let app = create_test_router();
    let server = TestServer::new(app).unwrap();
    
    let response = server
        .get("/test")
        .add_header(
            HeaderName::from_static("x-custom-header"),
            HeaderValue::from_static("custom-value")
        )
        .add_header(
            HeaderName::from_static("authorization"),
            HeaderValue::from_static("Bearer-token123")
        )
        .await;
    
    assert_eq!(response.status_code(), StatusCode::OK);
    verify_security_headers(&response);
}

#[tokio::test]
async fn test_content_security_policy_strictness() {
    let app = create_test_router();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/test").await;
    
    let csp_header = response.header("Content-Security-Policy");
    let csp_str = csp_header.to_str().unwrap();
    assert!(csp_str.contains("default-src 'none'"));
    assert!(csp_str.contains("frame-ancestors 'none'"));
    
    // Verify no unsafe directives are present
    assert!(!csp_str.contains("'unsafe-inline'"));
    assert!(!csp_str.contains("'unsafe-eval'"));
}

#[tokio::test]
async fn test_cache_control_headers() {
    let app = create_test_router();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/test").await;
    
    let cache_control = response.header("Cache-Control");
    let cache_str = cache_control.to_str().unwrap();
    assert!(cache_str.contains("no-store"));
    assert!(cache_str.contains("no-cache"));
    assert!(cache_str.contains("must-revalidate"));
    
    assert_eq!(response.header("Pragma"), "no-cache");
    assert_eq!(response.header("Expires"), "0");
}

#[tokio::test]
async fn test_server_header_removal() {
    let app = create_test_router();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/test").await;
    
    // Server header should be removed for security
    let server_header = response.headers().get("Server");
    assert!(server_header.is_none(), "Server header should be removed");
}

#[tokio::test]
async fn test_torpc_service_identification() {
    let app = create_test_router();
    let server = TestServer::new(app).unwrap();
    
    let response = server.get("/test").await;
    
    assert_eq!(response.header("X-Service"), "TorPC");
}

#[tokio::test]
async fn test_security_headers_persistence_across_requests() {
    let app = create_test_router();
    let server = TestServer::new(app).unwrap();
    
    // Make multiple requests to ensure headers are consistently applied
    for _ in 0..5 {
        let response = server.get("/test").await;
        verify_security_headers(&response);
    }
}

// Helper function to verify all expected security headers are present
fn verify_security_headers(response: &axum_test::TestResponse) {
    let headers = response.headers();
    
    // Verify all required security headers are present with correct values
    assert_eq!(
        headers.get("X-Content-Type-Options").unwrap(),
        "nosniff",
        "X-Content-Type-Options header missing or incorrect"
    );
    
    assert_eq!(
        headers.get("X-Frame-Options").unwrap(),
        "DENY",
        "X-Frame-Options header missing or incorrect"
    );
    
    assert_eq!(
        headers.get("X-XSS-Protection").unwrap(),
        "0",
        "X-XSS-Protection header missing or incorrect"
    );
    
    assert_eq!(
        headers.get("Referrer-Policy").unwrap(),
        "no-referrer",
        "Referrer-Policy header missing or incorrect"
    );
    
    let csp = headers.get("Content-Security-Policy").unwrap();
    let csp_str = csp.to_str().unwrap();
    assert!(
        csp_str.contains("default-src 'none'"),
        "Content-Security-Policy missing default-src 'none'"
    );
    assert!(
        csp_str.contains("frame-ancestors 'none'"),
        "Content-Security-Policy missing frame-ancestors 'none'"
    );
    
    let cache_control = headers.get("Cache-Control").unwrap();
    let cache_str = cache_control.to_str().unwrap();
    assert!(
        cache_str.contains("no-store"),
        "Cache-Control missing no-store"
    );
    assert!(
        cache_str.contains("no-cache"),
        "Cache-Control missing no-cache"
    );
    assert!(
        cache_str.contains("must-revalidate"),
        "Cache-Control missing must-revalidate"
    );
    
    assert_eq!(
        headers.get("Pragma").unwrap(),
        "no-cache",
        "Pragma header missing or incorrect"
    );
    
    assert_eq!(
        headers.get("Expires").unwrap(),
        "0",
        "Expires header missing or incorrect"
    );
    
    assert_eq!(
        headers.get("X-Service").unwrap(),
        "TorPC",
        "X-Service header missing or incorrect"
    );
    
    // Verify Server header is removed
    assert!(
        headers.get("Server").is_none(),
        "Server header should be removed for security"
    );
}

#[tokio::test]
async fn test_security_headers_with_large_response() {
    let app = Router::new()
        .route("/large", get(|| async { "x".repeat(10000) }))
        .layer(middleware::from_fn(security_headers_middleware));
    
    let server = TestServer::new(app).unwrap();
    let response = server.get("/large").await;
    
    assert_eq!(response.status_code(), StatusCode::OK);
    verify_security_headers(&response);
    
    // Verify response content is intact
    let body = response.text();
    assert_eq!(body.len(), 10000);
}

#[tokio::test]
async fn test_security_headers_with_empty_response() {
    let app = Router::new()
        .route("/empty", get(|| async { "" }))
        .layer(middleware::from_fn(security_headers_middleware));
    
    let server = TestServer::new(app).unwrap();
    let response = server.get("/empty").await;
    
    assert_eq!(response.status_code(), StatusCode::OK);
    verify_security_headers(&response);
}