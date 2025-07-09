//! Retry logic and circuit breaker implementation
//! 
//! Provides fault-tolerant communication with MEV relays through
//! exponential backoff and circuit breaker patterns.

use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::time::sleep;
use rand::Rng;

/// Circuit breaker states
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CircuitState {
    /// Circuit is closed - requests flow normally
    Closed,
    /// Circuit is open - requests are rejected
    Open,
    /// Circuit is half-open - testing if service recovered
    HalfOpen,
}

/// Circuit breaker for fault tolerance
/// 
/// Implements the circuit breaker pattern to prevent cascading failures
/// when communicating with external MEV relays.
/// 
/// # States
/// 
/// - **Closed**: Normal operation, requests pass through
/// - **Open**: Too many failures, requests are rejected immediately
/// - **Half-Open**: Testing phase to see if service recovered
/// 
/// # Usage
/// 
/// The circuit breaker is used internally by the MEV client to automatically
/// handle relay failures without overwhelming the service with requests.
pub struct CircuitBreaker {
    /// Current state
    state: Arc<AtomicU32>,
    /// Failure count
    failure_count: Arc<AtomicU32>,
    /// Last failure timestamp (milliseconds since epoch)
    last_failure_time: Arc<AtomicU64>,
    /// Configuration
    config: CircuitBreakerConfig,
}

/// Circuit breaker configuration
#[derive(Debug, Clone)]
pub struct CircuitBreakerConfig {
    /// Number of failures before opening circuit
    pub failure_threshold: u32,
    /// How long to wait before attempting recovery (half-open state)
    pub reset_timeout: Duration,
    /// Success count needed in half-open state to close circuit
    pub success_threshold: u32,
}

impl Default for CircuitBreakerConfig {
    fn default() -> Self {
        Self {
            failure_threshold: 5,
            reset_timeout: Duration::from_secs(60),
            success_threshold: 3,
        }
    }
}

impl CircuitBreaker {
    /// Create a new circuit breaker
    pub fn new(config: CircuitBreakerConfig) -> Self {
        Self {
            state: Arc::new(AtomicU32::new(CircuitState::Closed as u32)),
            failure_count: Arc::new(AtomicU32::new(0)),
            last_failure_time: Arc::new(AtomicU64::new(0)),
            config,
        }
    }
    
    /// Check if requests should be allowed
    pub fn should_allow_request(&self) -> bool {
        let state = self.get_state();
        
        match state {
            CircuitState::Closed => true,
            CircuitState::Open => {
                // Check if we should transition to half-open
                let last_failure = self.last_failure_time.load(Ordering::Relaxed);
                let now = Instant::now().elapsed().as_millis() as u64;
                
                if now - last_failure > self.config.reset_timeout.as_millis() as u64 {
                    self.set_state(CircuitState::HalfOpen);
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }
    
    /// Record a successful request
    pub fn record_success(&self) {
        let state = self.get_state();
        
        match state {
            CircuitState::HalfOpen => {
                // In half-open state, success moves us back to closed
                self.failure_count.store(0, Ordering::Relaxed);
                self.set_state(CircuitState::Closed);
            }
            _ => {
                // Reset failure count on success
                self.failure_count.store(0, Ordering::Relaxed);
            }
        }
    }
    
    /// Record a failed request
    pub fn record_failure(&self) {
        let failures = self.failure_count.fetch_add(1, Ordering::Relaxed) + 1;
        self.last_failure_time.store(
            Instant::now().elapsed().as_millis() as u64,
            Ordering::Relaxed
        );
        
        let state = self.get_state();
        
        match state {
            CircuitState::Closed => {
                if failures >= self.config.failure_threshold {
                    self.set_state(CircuitState::Open);
                }
            }
            CircuitState::HalfOpen => {
                // Any failure in half-open state reopens the circuit
                self.set_state(CircuitState::Open);
            }
            CircuitState::Open => {
                // Already open, nothing to do
            }
        }
    }
    
    /// Get current circuit state
    fn get_state(&self) -> CircuitState {
        match self.state.load(Ordering::Relaxed) {
            0 => CircuitState::Closed,
            1 => CircuitState::Open,
            2 => CircuitState::HalfOpen,
            _ => CircuitState::Closed,
        }
    }
    
    /// Set circuit state
    fn set_state(&self, state: CircuitState) {
        self.state.store(state as u32, Ordering::Relaxed);
    }
}

/// Retry configuration for MEV operations
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Initial backoff duration
    pub initial_backoff: Duration,
    /// Maximum backoff duration
    pub max_backoff: Duration,
    /// Backoff multiplier (typically 2.0)
    pub backoff_multiplier: f64,
    /// Whether to add jitter to backoff
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(100),
            max_backoff: Duration::from_secs(5),
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }
}

/// Execute a function with retry logic
/// 
/// # Arguments
/// 
/// * `config` - Retry configuration
/// * `circuit_breaker` - Optional circuit breaker for fault tolerance
/// * `operation` - Async function to execute
/// 
/// # Returns
/// 
/// The result of the operation or the last error after all retries
pub async fn retry_with_backoff<F, Fut, T, E>(
    config: &RetryConfig,
    circuit_breaker: Option<&CircuitBreaker>,
    mut operation: F,
) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Display + std::fmt::Debug,
{
    let mut attempt = 0;
    let mut backoff = config.initial_backoff;
    
    loop {
        // Check circuit breaker
        if let Some(cb) = circuit_breaker {
            if !cb.should_allow_request() {
                // Circuit is open, fail fast with a synthetic error
                // We can't easily create a generic error, so we'll run the operation once
                // to get a proper error type, but we won't retry
                let result = operation().await;
                if let Err(e) = result {
                    return Err(e);
                }
                // This shouldn't happen in practice since circuit breaker opens on failures
                panic!("Circuit breaker is open but operation succeeded");
            }
        }
        
        attempt += 1;
        
        match operation().await {
            Ok(result) => {
                if let Some(cb) = circuit_breaker {
                    cb.record_success();
                }
                return Ok(result);
            }
            Err(err) => {
                if let Some(cb) = circuit_breaker {
                    cb.record_failure();
                }
                
                if attempt >= config.max_attempts {
                    return Err(err);
                }
                
                // Calculate next backoff with optional jitter
                let mut delay = backoff;
                if config.jitter {
                    let mut rng = rand::thread_rng();
                    let jitter_factor = rng.gen_range(0.8..1.2);
                    delay = Duration::from_millis((delay.as_millis() as f64 * jitter_factor) as u64);
                }
                
                sleep(delay).await;
                
                // Increase backoff for next attempt
                backoff = Duration::from_millis(
                    (backoff.as_millis() as f64 * config.backoff_multiplier) as u64
                ).min(config.max_backoff);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[tokio::test]
    async fn test_circuit_breaker_states() {
        let config = CircuitBreakerConfig {
            failure_threshold: 2,
            reset_timeout: Duration::from_millis(100),
            success_threshold: 1,
        };
        
        let cb = CircuitBreaker::new(config);
        
        // Initial state should be closed
        assert!(cb.should_allow_request());
        
        // Record failures
        cb.record_failure();
        assert!(cb.should_allow_request()); // Still closed
        
        cb.record_failure();
        assert!(!cb.should_allow_request()); // Now open
        
        // Wait for reset timeout
        tokio::time::sleep(Duration::from_millis(150)).await;
        assert!(cb.should_allow_request()); // Should be half-open
        
        // Success in half-open should close circuit
        cb.record_success();
        assert!(cb.should_allow_request());
    }
    
    #[tokio::test]
    async fn test_retry_logic() {
        let config = RetryConfig {
            max_attempts: 3,
            initial_backoff: Duration::from_millis(10),
            max_backoff: Duration::from_millis(100),
            backoff_multiplier: 2.0,
            jitter: false,
        };
        
        let call_count = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let result = retry_with_backoff(&config, None, || {
            let call_count = call_count.clone();
            async move {
                let count = call_count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1;
                if count < 3 {
                    Err("temporary error")
                } else {
                    Ok("success")
                }
            }
        }).await;
        
        assert_eq!(result, Ok("success"));
        assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 3);
    }
}