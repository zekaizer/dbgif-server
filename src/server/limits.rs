use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{RwLock, Semaphore, TryAcquireError};
use tokio::time::interval;
use tracing::{debug, error, info, warn};

/// Connection rate limiting and backpressure management
pub struct ConnectionLimiter {
    /// Maximum number of concurrent connections
    max_connections: usize,
    /// Semaphore for connection limiting
    connection_semaphore: Arc<Semaphore>,
    /// Rate limiting configuration
    rate_limit_config: RateLimitConfig,
    /// Connection statistics
    stats: Arc<RwLock<ConnectionStats>>,
}

/// Rate limiting configuration
#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    /// Maximum connections per second
    pub max_connections_per_second: u32,
    /// Maximum connections per minute
    pub max_connections_per_minute: u32,
    /// Connection burst allowance
    pub burst_allowance: u32,
    /// Backpressure threshold (percentage of max connections)
    pub backpressure_threshold: f32,
    /// Client ban duration for rate limit violations
    pub ban_duration: Duration,
}

impl Default for RateLimitConfig {
    fn default() -> Self {
        Self {
            max_connections_per_second: 10,
            max_connections_per_minute: 100,
            burst_allowance: 20,
            backpressure_threshold: 0.8, // 80%
            ban_duration: Duration::from_secs(60), // 1 minute
        }
    }
}

/// Connection statistics with token bucket rate limiting
#[derive(Debug)]
pub struct ConnectionStats {
    /// Total connection attempts
    pub total_attempts: u64,
    /// Successful connections
    pub successful_connections: u64,
    /// Rejected connections (rate limited)
    pub rejected_connections: u64,
    /// Currently active connections
    pub active_connections: u32,
    /// Token bucket for per-second rate limiting
    pub tokens_per_second: u32,
    /// Token bucket for per-minute rate limiting
    pub tokens_per_minute: u32,
    /// Last token refill time
    pub last_token_refill: Instant,
    /// Backpressure active
    pub backpressure_active: bool,
}

impl Default for ConnectionStats {
    fn default() -> Self {
        Self {
            total_attempts: 0,
            successful_connections: 0,
            rejected_connections: 0,
            active_connections: 0,
            tokens_per_second: 0, // Will be initialized by ConnectionLimiter
            tokens_per_minute: 0, // Will be initialized by ConnectionLimiter
            last_token_refill: Instant::now(),
            backpressure_active: false,
        }
    }
}

/// Rate limiting error types
#[derive(Debug, thiserror::Error)]
pub enum RateLimitError {
    #[error("Connection limit exceeded: {current}/{max} connections")]
    ConnectionLimitExceeded { current: usize, max: usize },

    #[error("Rate limit exceeded: {rate} connections per {window}")]
    RateLimitExceeded { rate: u32, window: String },

    #[error("Client temporarily banned until {until:?}")]
    ClientBanned { until: Instant },

    #[error("Backpressure active: server overloaded")]
    BackpressureActive,

    #[error("Connection rejected: {reason}")]
    ConnectionRejected { reason: String },
}

/// Result type for rate limiting operations
pub type RateLimitResult<T> = Result<T, RateLimitError>;

/// Connection permit that must be held while connection is active
pub struct ConnectionPermit {
    /// Semaphore permit
    _permit: tokio::sync::OwnedSemaphorePermit,
    /// Connection start time
    start_time: Instant,
    /// Connection ID for tracking
    connection_id: String,
    /// Reference to stats for cleanup
    stats: Arc<RwLock<ConnectionStats>>,
}

impl ConnectionPermit {
    /// Get connection duration
    pub fn duration(&self) -> Duration {
        self.start_time.elapsed()
    }

    /// Get connection ID
    pub fn connection_id(&self) -> &str {
        &self.connection_id
    }
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        // Update stats when connection is closed
        let stats = self.stats.clone();
        let duration = self.duration();

        tokio::spawn(async move {
            let mut stats_guard = stats.write().await;
            stats_guard.active_connections = stats_guard.active_connections.saturating_sub(1);
            debug!("Connection closed after {:?}, active: {}", duration, stats_guard.active_connections);
        });
    }
}

/// Client tracking for rate limiting
#[derive(Debug)]
pub struct ClientTracker {
    /// Client IP or identifier
    pub client_id: String,
    /// Connection attempts in current window
    pub connections_this_second: u32,
    pub connections_this_minute: u32,
    /// Last connection attempt time
    pub last_connection: Instant,
    /// Ban expiry time (if banned)
    pub banned_until: Option<Instant>,
    /// Total connections from this client
    pub total_connections: u64,
}

impl ConnectionLimiter {
    /// Create a new connection limiter
    pub fn new(max_connections: usize, config: RateLimitConfig) -> Self {
        let mut stats = ConnectionStats::default();
        // Initialize token buckets with full capacity
        stats.tokens_per_second = config.max_connections_per_second;
        stats.tokens_per_minute = config.max_connections_per_minute;

        Self {
            max_connections,
            connection_semaphore: Arc::new(Semaphore::new(max_connections)),
            rate_limit_config: config,
            stats: Arc::new(RwLock::new(stats)),
        }
    }

    /// Try to acquire a connection permit
    pub async fn try_acquire(&self, client_id: String) -> RateLimitResult<ConnectionPermit> {
        // Update statistics
        {
            let mut stats = self.stats.write().await;
            stats.total_attempts += 1;
        }

        // Check rate limits
        self.check_rate_limits(&client_id).await?;

        // Check backpressure
        self.check_backpressure().await?;

        // Try to acquire semaphore permit
        let permit = match self.connection_semaphore.clone().try_acquire_owned() {
            Ok(permit) => permit,
            Err(TryAcquireError::NoPermits) => {
                let stats = self.stats.read().await;
                return Err(RateLimitError::ConnectionLimitExceeded {
                    current: stats.active_connections as usize,
                    max: self.max_connections,
                });
            }
            Err(TryAcquireError::Closed) => {
                return Err(RateLimitError::ConnectionRejected {
                    reason: "Connection limiter closed".to_string(),
                });
            }
        };

        // Update statistics
        {
            let mut stats = self.stats.write().await;
            stats.successful_connections += 1;
            stats.active_connections += 1;
        }

        info!("Connection permit acquired for client {}", client_id);

        Ok(ConnectionPermit {
            _permit: permit,
            start_time: Instant::now(),
            connection_id: client_id,
            stats: self.stats.clone(),
        })
    }

    /// Check rate limits using token bucket algorithm
    async fn check_rate_limits(&self, _client_id: &str) -> RateLimitResult<()> {
        let mut stats = self.stats.write().await;

        // Refill token buckets based on time elapsed
        self.refill_tokens(&mut stats).await;

        // Check if we have enough tokens for this connection
        if stats.tokens_per_second == 0 {
            return Err(RateLimitError::RateLimitExceeded {
                rate: self.rate_limit_config.max_connections_per_second,
                window: "second".to_string(),
            });
        }

        if stats.tokens_per_minute == 0 {
            return Err(RateLimitError::RateLimitExceeded {
                rate: self.rate_limit_config.max_connections_per_minute,
                window: "minute".to_string(),
            });
        }

        // Consume tokens
        stats.tokens_per_second -= 1;
        stats.tokens_per_minute -= 1;

        Ok(())
    }

    /// Refill token buckets based on elapsed time
    async fn refill_tokens(&self, stats: &mut ConnectionStats) {
        let now = Instant::now();
        let elapsed = now.duration_since(stats.last_token_refill);

        // Refill tokens for per-second bucket (1 token per second)
        let seconds_elapsed = elapsed.as_secs();
        if seconds_elapsed > 0 {
            let tokens_to_add = seconds_elapsed.min(self.rate_limit_config.max_connections_per_second as u64) as u32;
            stats.tokens_per_second = (stats.tokens_per_second + tokens_to_add)
                .min(self.rate_limit_config.max_connections_per_second);
        }

        // Refill tokens for per-minute bucket (tokens refilled continuously)
        let minutes_elapsed = elapsed.as_secs() / 60;
        if minutes_elapsed > 0 {
            let tokens_to_add = (minutes_elapsed * self.rate_limit_config.max_connections_per_minute as u64 / 60)
                .min(self.rate_limit_config.max_connections_per_minute as u64) as u32;
            stats.tokens_per_minute = (stats.tokens_per_minute + tokens_to_add)
                .min(self.rate_limit_config.max_connections_per_minute);
        }

        // Update last refill time if any time has passed
        if elapsed >= Duration::from_secs(1) {
            stats.last_token_refill = now;
        }
    }

    /// Check if backpressure should be applied
    async fn check_backpressure(&self) -> RateLimitResult<()> {
        let available_permits = self.connection_semaphore.available_permits();
        let utilization = (self.max_connections - available_permits) as f32 / self.max_connections as f32;

        if utilization >= self.rate_limit_config.backpressure_threshold {
            return Err(RateLimitError::BackpressureActive);
        }

        Ok(())
    }

    /// Start background task for token bucket refill (no longer needed with continuous refill)
    pub fn start_rate_limit_reset_task(&self) -> tokio::task::JoinHandle<()> {
        let stats = self.stats.clone();
        let rate_limit_config = self.rate_limit_config.clone();

        tokio::spawn(async move {
            let mut interval = interval(Duration::from_secs(1));

            loop {
                interval.tick().await;

                let mut stats_guard = stats.write().await;
                let now = Instant::now();
                let elapsed = now.duration_since(stats_guard.last_token_refill);

                // Refill tokens for per-second bucket
                let seconds_elapsed = elapsed.as_secs();
                if seconds_elapsed > 0 {
                    let tokens_to_add = seconds_elapsed.min(rate_limit_config.max_connections_per_second as u64) as u32;
                    stats_guard.tokens_per_second = (stats_guard.tokens_per_second + tokens_to_add)
                        .min(rate_limit_config.max_connections_per_second);
                }

                // Refill tokens for per-minute bucket
                let minutes_elapsed = elapsed.as_secs() / 60;
                if minutes_elapsed > 0 {
                    let tokens_to_add = (minutes_elapsed * rate_limit_config.max_connections_per_minute as u64 / 60)
                        .min(rate_limit_config.max_connections_per_minute as u64) as u32;
                    stats_guard.tokens_per_minute = (stats_guard.tokens_per_minute + tokens_to_add)
                        .min(rate_limit_config.max_connections_per_minute);
                }

                if elapsed >= Duration::from_secs(1) {
                    stats_guard.last_token_refill = now;
                }

                // No need to reset counters since we use token buckets now
            }
        })
    }

    /// Get current connection statistics
    pub async fn get_stats(&self) -> ConnectionStats {
        let stats = self.stats.read().await;
        ConnectionStats {
            total_attempts: stats.total_attempts,
            successful_connections: stats.successful_connections,
            rejected_connections: stats.rejected_connections,
            active_connections: stats.active_connections,
            tokens_per_second: stats.tokens_per_second,
            tokens_per_minute: stats.tokens_per_minute,
            last_token_refill: stats.last_token_refill,
            backpressure_active: stats.backpressure_active,
        }
    }

    /// Update backpressure status
    pub async fn update_backpressure_status(&self) {
        let mut stats = self.stats.write().await;
        let utilization = stats.active_connections as f32 / self.max_connections as f32;
        stats.backpressure_active = utilization >= self.rate_limit_config.backpressure_threshold;
    }

    /// Get available connection slots
    pub async fn available_permits(&self) -> usize {
        self.connection_semaphore.available_permits()
    }

    /// Force close connections (emergency)
    pub async fn emergency_close(&self, count: usize) -> usize {
        warn!("Emergency closing {} connections due to resource exhaustion", count);

        // This is a simplified implementation
        // In practice, you'd want to close specific connections based on criteria
        let mut closed = 0;
        for _ in 0..count {
            if self.connection_semaphore.available_permits() < self.max_connections {
                // Signal that a connection should be closed
                // Implementation depends on how connections are managed
                closed += 1;
            } else {
                break;
            }
        }

        {
            let mut stats = self.stats.write().await;
            stats.active_connections = stats.active_connections.saturating_sub(closed as u32);
        }

        closed
    }
}

/// Backpressure manager for handling server overload
pub struct BackpressureManager {
    /// Connection limiter
    limiter: Arc<ConnectionLimiter>,
    /// Backpressure configuration
    config: BackpressureConfig,
}

/// Backpressure configuration
#[derive(Debug, Clone)]
pub struct BackpressureConfig {
    /// CPU usage threshold for backpressure
    pub cpu_threshold: f32,
    /// Memory usage threshold for backpressure
    pub memory_threshold: f32,
    /// Connection queue length threshold
    pub queue_length_threshold: usize,
    /// Backpressure check interval
    pub check_interval: Duration,
}

impl Default for BackpressureConfig {
    fn default() -> Self {
        Self {
            cpu_threshold: 0.8,       // 80% CPU
            memory_threshold: 0.85,   // 85% memory
            queue_length_threshold: 1000,
            check_interval: Duration::from_secs(5),
        }
    }
}

impl BackpressureManager {
    /// Create a new backpressure manager
    pub fn new(limiter: Arc<ConnectionLimiter>, config: BackpressureConfig) -> Self {
        Self {
            limiter,
            config,
        }
    }

    /// Start backpressure monitoring task
    pub fn start_monitoring_task(&self) -> tokio::task::JoinHandle<()> {
        let limiter = self.limiter.clone();
        let config = self.config.clone();

        tokio::spawn(async move {
            let mut interval = interval(config.check_interval);

            loop {
                interval.tick().await;

                // Check system resources and update backpressure status
                let should_apply_backpressure = Self::should_apply_backpressure(&config).await;

                if should_apply_backpressure {
                    warn!("Applying backpressure due to resource constraints");
                    limiter.update_backpressure_status().await;
                }
            }
        })
    }

    /// Check if backpressure should be applied based on system resources
    async fn should_apply_backpressure(_config: &BackpressureConfig) -> bool {
        // Simplified resource checking
        // In a real implementation, you'd check actual CPU/memory usage

        // For now, just return false - real implementation would check:
        // - CPU usage via sysinfo or similar
        // - Memory usage
        // - Connection queue lengths
        // - Other system metrics

        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::sleep;

    #[tokio::test]
    async fn test_connection_limit() {
        let config = RateLimitConfig {
            backpressure_threshold: 0.99, // Disable backpressure for this test
            ..RateLimitConfig::default()
        };
        let limiter = ConnectionLimiter::new(2, config);

        // Should be able to acquire 2 permits
        let permit1 = limiter.try_acquire("client1".to_string()).await.unwrap();
        let _permit2 = limiter.try_acquire("client2".to_string()).await.unwrap();

        // Third should fail
        let result = limiter.try_acquire("client3".to_string()).await;
        assert!(result.is_err());

        drop(permit1);

        // Now should succeed again
        let _permit3 = limiter.try_acquire("client3".to_string()).await.unwrap();
    }

    #[tokio::test]
    async fn test_rate_limit_reset() {
        let limiter = ConnectionLimiter::new(100, RateLimitConfig {
            max_connections_per_second: 1,
            backpressure_threshold: 0.99, // Disable backpressure for this test
            ..RateLimitConfig::default()
        });

        // Start reset task
        let _reset_task = limiter.start_rate_limit_reset_task();

        // First connection should succeed
        let _permit1 = limiter.try_acquire("client1".to_string()).await.unwrap();

        // Second connection should fail due to rate limit
        let result = limiter.try_acquire("client2".to_string()).await;
        assert!(matches!(result, Err(RateLimitError::RateLimitExceeded { .. })));

        // Wait for rate limit reset
        sleep(Duration::from_millis(1100)).await;

        // Should succeed after reset
        let _permit2 = limiter.try_acquire("client2".to_string()).await.unwrap();
    }

    #[tokio::test]
    async fn test_connection_stats() {
        let limiter = ConnectionLimiter::new(10, RateLimitConfig::default());

        let initial_stats = limiter.get_stats().await;
        assert_eq!(initial_stats.active_connections, 0);
        assert_eq!(initial_stats.total_attempts, 0);

        let _permit = limiter.try_acquire("client1".to_string()).await.unwrap();

        let stats = limiter.get_stats().await;
        assert_eq!(stats.active_connections, 1);
        assert_eq!(stats.total_attempts, 1);
        assert_eq!(stats.successful_connections, 1);
    }

    #[tokio::test]
    async fn test_backpressure_threshold() {
        let config = RateLimitConfig {
            backpressure_threshold: 0.5, // 50%
            ..RateLimitConfig::default()
        };
        let limiter = ConnectionLimiter::new(4, config); // Use 4 slots so 50% = 2 slots

        // Fill up to just under backpressure threshold (1/4 = 25%)
        let _permit1 = limiter.try_acquire("client1".to_string()).await.unwrap();

        // Should still work (2/4 = 50% exactly)
        let _permit2 = limiter.try_acquire("client2".to_string()).await.unwrap();

        // Now at 50% exactly, should trigger backpressure for next one
        let result = limiter.try_acquire("client3".to_string()).await;
        assert!(matches!(result, Err(RateLimitError::BackpressureActive)));
    }
}