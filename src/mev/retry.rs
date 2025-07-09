//! Retry logic and circuit breaker for MEV relay resilience
//! 
//! This module provides fault tolerance mechanisms to handle
//! transient failures and prevent cascading failures when
//! communicating with MEV relays.

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::Mutex;
use rand::{thread_rng, Rng};
use tracing::{debug, warn};

use super::types::CircuitState;

/// Implements exponential backoff with jitter for retrying failed requests
/// 
/// # For Library Developers
/// Used internally by MevRelayClient to handle transient failures.
/// Not exposed in public API.
/// 
/// # Retry Strategy
/// - Initial delay: 100ms
/// - Max delay: 5s  
/// - Jitter: ±25% to prevent thundering herd
/// - Max attempts: 3
/// 
/// # Example
/// ```rust
/// let policy = RetryPolicy::default();
/// for attempt in 0..policy.max_attempts {
///     match make_request().await {
///         Ok(response) => return Ok(response),
///         Err(e) if policy.should_retry(&e) => {
///             let delay = policy.calculate_delay(attempt);
///             tokio::time::sleep(delay).await;
///         }
///         Err(e) => return Err(e),
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct RetryPolicy {
    /// Maximum number of retry attempts
    pub max_attempts: u32,
    /// Initial delay before first retry
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Jitter factor (0.0 to 1.0)
    pub jitter_factor: f64,
}

impl Default for RetryPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(5),
            jitter_factor: 0.25,
        }
    }
}

impl RetryPolicy {
    /// Calculate delay for a given attempt number with exponential backoff
    /// 
    /// # Arguments
    /// * `attempt` - Zero-based attempt number
    /// 
    /// # Returns
    /// Duration to wait before next attempt
    pub fn calculate_delay(&self, attempt: u32) -> Duration {
        // Exponential backoff: delay * 2^attempt
        let base_delay = self.initial_delay.as_millis() as f64 * (2_u32.pow(attempt) as f64);
        
        // Cap at max delay
        let capped_delay = base_delay.min(self.max_delay.as_millis() as f64);
        
        // Add jitter (±jitter_factor)
        let mut rng = thread_rng();
        let jitter_range = capped_delay * self.jitter_factor;
        let jitter = rng.gen_range(-jitter_range..=jitter_range);
        let final_delay = (capped_delay + jitter).max(0.0) as u64;
        
        Duration::from_millis(final_delay)
    }
    
    /// Determine if an error is retryable
    /// 
    /// # For Library Developers
    /// Network errors and timeouts are retryable.
    /// Authentication and validation errors are not.
    pub fn should_retry(&self, error: &str) -> bool {
        // Don't retry authentication errors
        if error.contains("authentication") || error.contains("unauthorized") {
            return false;
        }
        
        // Don't retry validation errors
        if error.contains("invalid") || error.contains("malformed") {
            return false;
        }
        
        // Retry network and timeout errors
        if error.contains("timeout") || 
           error.contains("connection") || 
           error.contains("network") ||
           error.contains("temporarily unavailable") {
            return true;
        }
        
        // Default: retry on generic errors
        true
    }
}

/// Circuit breaker to prevent cascading failures
/// 
/// # For Library Developers
/// Tracks consecutive failures and temporarily disables requests
/// when a threshold is reached.
/// 
/// # State Transitions
/// - Closed -> Open: After 5 consecutive failures
/// - Open -> HalfOpen: After 30 seconds
/// - HalfOpen -> Closed: After 1 success
/// - HalfOpen -> Open: After 1 failure
/// 
/// # Example
/// ```rust
/// let breaker = CircuitBreaker::new();
/// 
/// if !breaker.can_proceed().await {
///     return Err("Circuit breaker open");
/// }
/// 
/// match make_request().await {
///     Ok(response) => {
///         breaker.record_success().await;
///         Ok(response)
///     }
///     Err(e) => {
///         breaker.record_failure().await;
///         Err(e)
///     }
/// }
/// ```
pub struct CircuitBreaker {
    /// Current state of the circuit
    state: Arc<Mutex<CircuitState>>,
    /// Number of failures before opening
    failure_threshold: u32,
    /// How long to wait before attempting recovery
    recovery_timeout: Duration,
}

impl CircuitBreaker {
    /// Create a new circuit breaker with default settings
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(CircuitState::Closed)),
            failure_threshold: 5,
            recovery_timeout: Duration::from_secs(30),
        }
    }
    
    /// Create a circuit breaker with custom settings
    /// 
    /// # Arguments
    /// * `failure_threshold` - Failures before opening circuit
    /// * `recovery_timeout` - Time to wait before half-open
    pub fn with_config(failure_threshold: u32, recovery_timeout: Duration) -> Self {
        Self {
            state: Arc::new(Mutex::new(CircuitState::Closed)),
            failure_threshold,
            recovery_timeout,
        }
    }
    
    /// Check if a request should be allowed to proceed
    /// 
    /// # Returns
    /// * `true` - Request can proceed
    /// * `false` - Circuit is open, fail fast
    pub async fn can_proceed(&self) -> bool {
        let mut state = self.state.lock().await;
        
        match &*state {
            CircuitState::Closed => true,
            CircuitState::Open { opened_at, .. } => {
                // Check if recovery timeout has elapsed
                if opened_at.elapsed() >= self.recovery_timeout {
                    debug!("Circuit breaker transitioning to half-open");
                    *state = CircuitState::HalfOpen;
                    true
                } else {
                    false
                }
            }
            CircuitState::HalfOpen => true,
        }
    }
    
    /// Record a successful request
    /// 
    /// Resets failure count and closes circuit if half-open
    pub async fn record_success(&self) {
        let mut state = self.state.lock().await;
        
        match &*state {
            CircuitState::HalfOpen => {
                debug!("Circuit breaker closing after successful recovery");
                *state = CircuitState::Closed;
            }
            _ => {
                // Success in closed state maintains closed
                *state = CircuitState::Closed;
            }
        }
    }
    
    /// Record a failed request
    /// 
    /// May trigger circuit opening if threshold is reached
    pub async fn record_failure(&self) {
        let mut state = self.state.lock().await;
        
        match &*state {
            CircuitState::Closed => {
                // First failure, start counting
                *state = CircuitState::Open {
                    opened_at: Instant::now(),
                    failure_count: 1,
                };
                
                // If we haven't reached threshold, immediately close again
                if 1 < self.failure_threshold {
                    *state = CircuitState::Closed;
                } else {
                    warn!("Circuit breaker opened after {} failures", 1);
                }
            }
            CircuitState::Open { opened_at, failure_count } => {
                let new_count = failure_count + 1;
                if new_count >= self.failure_threshold && opened_at.elapsed() < Duration::from_secs(1) {
                    // Keep open with updated count
                    *state = CircuitState::Open {
                        opened_at: *opened_at,
                        failure_count: new_count,
                    };
                }
            }
            CircuitState::HalfOpen => {
                // Failed during recovery, reopen
                warn!("Circuit breaker reopening after failed recovery attempt");
                *state = CircuitState::Open {
                    opened_at: Instant::now(),
                    failure_count: self.failure_threshold,
                };
            }
        }
    }
    
    /// Get current circuit state (for monitoring)
    pub async fn state(&self) -> CircuitState {
        self.state.lock().await.clone()
    }
}

/// Tracks consecutive failures for circuit breaker logic
/// 
/// # For Library Developers
/// This is a simpler alternative implementation that just tracks
/// consecutive failures without the full state machine.
pub struct ConsecutiveFailureTracker {
    failures: Arc<Mutex<u32>>,
    threshold: u32,
}

impl ConsecutiveFailureTracker {
    pub fn new(threshold: u32) -> Self {
        Self {
            failures: Arc::new(Mutex::new(0)),
            threshold,
        }
    }
    
    pub async fn record_success(&self) {
        *self.failures.lock().await = 0;
    }
    
    pub async fn record_failure(&self) -> bool {
        let mut failures = self.failures.lock().await;
        *failures += 1;
        *failures >= self.threshold
    }
    
    pub async fn should_open(&self) -> bool {
        *self.failures.lock().await >= self.threshold
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_retry_delay_calculation() {
        let policy = RetryPolicy::default();
        
        // Test exponential backoff
        let delay0 = policy.calculate_delay(0);
        let delay1 = policy.calculate_delay(1);
        let delay2 = policy.calculate_delay(2);
        
        // Each delay should be roughly double the previous (minus jitter)
        assert!(delay0.as_millis() >= 75);  // 100ms - 25%
        assert!(delay0.as_millis() <= 125); // 100ms + 25%
        
        assert!(delay1.as_millis() >= 150);  // 200ms - 25%
        assert!(delay1.as_millis() <= 250); // 200ms + 25%
        
        assert!(delay2.as_millis() >= 300);  // 400ms - 25%
        assert!(delay2.as_millis() <= 500); // 400ms + 25%
    }
    
    #[test]
    fn test_max_delay_cap() {
        let policy = RetryPolicy::default();
        
        // Very high attempt number should cap at max_delay
        let delay = policy.calculate_delay(10);
        assert!(delay.as_millis() <= 6250); // 5000ms + 25%
    }
    
    #[test]
    fn test_should_retry() {
        let policy = RetryPolicy::default();
        
        // Retryable errors
        assert!(policy.should_retry("connection timeout"));
        assert!(policy.should_retry("network error"));
        assert!(policy.should_retry("temporarily unavailable"));
        
        // Non-retryable errors
        assert!(!policy.should_retry("authentication failed"));
        assert!(!policy.should_retry("unauthorized"));
        assert!(!policy.should_retry("invalid request"));
        assert!(!policy.should_retry("malformed JSON"));
    }
    
    #[tokio::test]
    async fn test_circuit_breaker_state_transitions() {
        let breaker = CircuitBreaker::with_config(2, Duration::from_millis(100));
        
        // Initially closed
        assert!(breaker.can_proceed().await);
        
        // First failure - still closed (threshold is 2)
        breaker.record_failure().await;
        assert!(breaker.can_proceed().await);
        
        // Second failure - should open
        breaker.record_failure().await;
        assert!(!breaker.can_proceed().await);
        
        // Wait for recovery timeout
        tokio::time::sleep(Duration::from_millis(150)).await;
        
        // Should transition to half-open
        assert!(breaker.can_proceed().await);
        
        // Success in half-open closes circuit
        breaker.record_success().await;
        assert!(matches!(
            breaker.state().await,
            CircuitState::Closed
        ));
    }
    
    #[tokio::test]
    async fn test_failure_tracker() {
        let tracker = ConsecutiveFailureTracker::new(3);
        
        // Record failures
        assert!(!tracker.record_failure().await); // 1
        assert!(!tracker.record_failure().await); // 2
        assert!(tracker.record_failure().await);  // 3 - threshold reached
        
        // Success resets counter
        tracker.record_success().await;
        assert!(!tracker.should_open().await);
    }
}