use axum::{
    extract::{ConnectInfo, Request, State},
    http::StatusCode,
    middleware::Next,
    response::Response,
};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::{Duration, Instant},
};
use tokio::sync::Mutex;
use tracing::warn;

/// Rate limiting configuration
#[derive(Clone, Debug)]
pub struct RateLimitConfig {
    /// Maximum requests per window
    pub max_requests: u32,
    /// Time window duration
    pub window_duration: Duration,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_requests: 100,
            window_duration: Duration::from_secs(60), // 100 requests per minute
        }
    }
}

/// Rate limiter state
pub struct RateLimiter {
    config: RateLimitConfig,
    /// Map of IP/Circuit ID to request count and window start
    requests: Arc<Mutex<HashMap<String, (u32, Instant)>>>,
}

impl RateLimiter {
    pub fn new(config: RateLimitConfig) -> Self {
        Self {
            config,
            requests: Arc::new(Mutex::new(HashMap::new())),
        }
    }
    
    /// Check if a request should be allowed
    pub async fn check_rate_limit(&self, identifier: &str) -> bool {
        let mut requests = self.requests.lock().await;
        let now = Instant::now();
        
        match requests.get_mut(identifier) {
            Some((count, window_start)) => {
                // Check if we're still in the same window
                if now.duration_since(*window_start) < self.config.window_duration {
                    if *count >= self.config.max_requests {
                        return false; // Rate limit exceeded
                    }
                    *count += 1;
                } else {
                    // New window, reset counter
                    *count = 1;
                    *window_start = now;
                }
            }
            None => {
                // First request from this identifier
                requests.insert(identifier.to_string(), (1, now));
            }
        }
        
        true
    }
    
    /// Clean up old entries (call periodically)
    pub async fn cleanup(&self) {
        let mut requests = self.requests.lock().await;
        let now = Instant::now();
        
        // Remove entries older than 2x the window duration
        let cutoff = self.config.window_duration * 2;
        requests.retain(|_, (_, window_start)| {
            now.duration_since(*window_start) < cutoff
        });
    }
}

/// Extract a per-connection identifier from the request. Tor terminates each
/// circuit as a fresh TCP connection from `127.0.0.1` with a unique ephemeral
/// source port, so port-based bucketing approximates per-circuit rate
/// limiting — far better than the previous behaviour, where every Tor user
/// shared a single global bucket and one attacker could trip the limit for
/// everyone.
///
/// The phantom `X-Tor-Circuit-ID` header check the old code performed has
/// been removed: Tor doesn't add such a header, so the branch was dead code
/// disguised as functionality.
fn get_request_identifier(req: &Request) -> String {
    if let Some(ConnectInfo(addr)) = req.extensions().get::<ConnectInfo<SocketAddr>>() {
        return format!("{}:{}", addr.ip(), addr.port());
    }

    // Fallback for tests / setups where ConnectInfo isn't wired up.
    "unknown".to_string()
}

/// Rate limiting middleware. Logs the bucket identifier on rejection so
/// operators can see whether a flood is from a single Tor circuit (one port)
/// or distributed (many ports) — useful when tuning thresholds.
pub async fn rate_limit_middleware(
    State(limiter): State<Arc<RateLimiter>>,
    req: Request,
    next: Next,
) -> Result<Response, StatusCode> {
    let identifier = get_request_identifier(&req);

    if !limiter.check_rate_limit(&identifier).await {
        warn!(identifier = %identifier, "rate limit exceeded");
        return Err(StatusCode::TOO_MANY_REQUESTS);
    }

    Ok(next.run(req).await)
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_rate_limiter_allows_under_limit() {
        let config = RateLimitConfig {
            max_requests: 5,
            window_duration: Duration::from_secs(1),
        };
        let limiter = RateLimiter::new(config);
        
        // Should allow up to 5 requests
        for i in 0..5 {
            assert!(
                limiter.check_rate_limit("test").await,
                "Request {} should be allowed", i + 1
            );
        }
    }
    
    #[tokio::test]
    async fn test_rate_limiter_blocks_over_limit() {
        let config = RateLimitConfig {
            max_requests: 3,
            window_duration: Duration::from_secs(1),
        };
        let limiter = RateLimiter::new(config);
        
        // Allow first 3 requests
        for _ in 0..3 {
            assert!(limiter.check_rate_limit("test").await);
        }
        
        // 4th request should be blocked
        assert!(!limiter.check_rate_limit("test").await);
    }
    
    #[tokio::test]
    async fn test_rate_limiter_resets_after_window() {
        let config = RateLimitConfig {
            max_requests: 2,
            window_duration: Duration::from_millis(100),
        };
        let limiter = RateLimiter::new(config);
        
        // Use up the limit
        assert!(limiter.check_rate_limit("test").await);
        assert!(limiter.check_rate_limit("test").await);
        assert!(!limiter.check_rate_limit("test").await);
        
        // Wait for window to expire
        tokio::time::sleep(Duration::from_millis(150)).await;
        
        // Should be allowed again
        assert!(limiter.check_rate_limit("test").await);
    }
    
    #[tokio::test]
    async fn test_rate_limiter_different_identifiers() {
        let config = RateLimitConfig {
            max_requests: 1,
            window_duration: Duration::from_secs(1),
        };
        let limiter = RateLimiter::new(config);
        
        // Different identifiers should have separate limits
        assert!(limiter.check_rate_limit("user1").await);
        assert!(limiter.check_rate_limit("user2").await);
        assert!(limiter.check_rate_limit("user3").await);
        
        // But each is limited individually
        assert!(!limiter.check_rate_limit("user1").await);
        assert!(!limiter.check_rate_limit("user2").await);
    }
    
    #[tokio::test]
    async fn test_cleanup_removes_old_entries() {
        let config = RateLimitConfig {
            max_requests: 1,
            window_duration: Duration::from_millis(50),
        };
        let limiter = RateLimiter::new(config);
        
        // Add some entries
        assert!(limiter.check_rate_limit("old").await);
        assert!(limiter.check_rate_limit("new").await);
        
        // Wait for entries to become old
        tokio::time::sleep(Duration::from_millis(150)).await;
        
        // Add a fresh entry
        assert!(limiter.check_rate_limit("fresh").await);
        
        // Run cleanup
        limiter.cleanup().await;
        
        // Check internal state
        let requests = limiter.requests.lock().await;
        assert!(!requests.contains_key("old"));
        assert!(!requests.contains_key("new"));
        assert!(requests.contains_key("fresh"));
    }
}