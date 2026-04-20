use dashmap::DashMap;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// A sliding-window rate limiter backed by `DashMap`.
///
/// Each key (e.g. client IP) independently tracks request timestamps.
/// When a request arrives, stale entries older than the configured window
/// are evicted and the remaining count is checked against `max_requests`.
#[derive(Debug, Clone)]
pub struct RateLimiter {
    max_requests: usize,
    window: Duration,
    entries: dashmap::DashMap<String, VecDeque<Instant>>,
}

impl RateLimiter {
    /// Create a new rate limiter.
    ///
    /// # Panics
    /// Panics if `max_requests` is 0.
    pub fn new(max_requests: usize, window: Duration) -> Self {
        assert!(max_requests > 0, "max_requests must be > 0");
        Self {
            max_requests,
            window,
            entries: DashMap::new(),
        }
    }

    /// Returns `true` if the request identified by `key` is allowed,
    /// `false` if it should be rejected (rate limit exceeded).
    ///
    /// This method is thread-safe and can be called concurrently.
    pub fn allow(&self, key: &str) -> bool {
        let now = Instant::now();
        let cutoff = now - self.window;

        let mut entry = self.entries.entry(key.to_string()).or_default();

        // Drain timestamps that have fallen outside the window
        let _before = entry.len();
        entry.retain(|&ts| ts > cutoff);

        if entry.len() < self.max_requests {
            entry.push_back(now);
            true
        } else {
            false
        }
    }

    /// Returns an approximate count of currently tracked entries (keys).
    pub fn key_count(&self) -> usize {
        self.entries.len()
    }

    /// Remove all timestamps for a given key.
    pub fn reset(&self, key: &str) {
        self.entries.remove(key);
    }

    /// Remove all rate-limit state.
    pub fn clear(&self) {
        self.entries.clear();
    }

    /// Returns the configured max_requests.
    pub fn max_requests(&self) -> usize {
        self.max_requests
    }

    /// Returns the configured window duration.
    pub fn window(&self) -> Duration {
        self.window
    }

    /// Evict stale entries (keys whose newest timestamp is outside the window).
    /// Useful for periodic cleanup to bound memory usage.
    pub fn evict_stale(&self) {
        let cutoff = Instant::now() - self.window;
        self.entries.retain(|_k, v| {
            v.retain(|&ts| ts > cutoff);
            !v.is_empty()
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    #[test]
    fn test_allow_within_limit() {
        let rl = RateLimiter::new(3, Duration::from_secs(60));
        assert!(rl.allow("a"));
        assert!(rl.allow("a"));
        assert!(rl.allow("a"));
    }

    #[test]
    fn test_reject_over_limit() {
        let rl = RateLimiter::new(3, Duration::from_secs(60));
        assert!(rl.allow("b"));
        assert!(rl.allow("b"));
        assert!(rl.allow("b"));
        assert!(!rl.allow("b")); // 4th should be rejected
        assert!(!rl.allow("b")); // 5th should also be rejected
    }

    #[test]
    fn test_different_keys_independent() {
        let rl = RateLimiter::new(2, Duration::from_secs(60));
        assert!(rl.allow("x"));
        assert!(rl.allow("x"));
        assert!(!rl.allow("x"));
        // Different key should still be allowed
        assert!(rl.allow("y"));
        assert!(rl.allow("y"));
        assert!(!rl.allow("y"));
    }

    #[test]
    fn test_max_requests_zero_panics() {
        let result = std::panic::catch_unwind(|| {
            RateLimiter::new(0, Duration::from_secs(60));
        });
        assert!(result.is_err());
    }

    #[test]
    fn test_reset_clears_key() {
        let rl = RateLimiter::new(2, Duration::from_secs(60));
        rl.allow("c");
        rl.allow("c");
        assert!(!rl.allow("c"));
        rl.reset("c");
        assert!(rl.allow("c")); // should be allowed again
    }

    #[test]
    fn test_clear_all() {
        let rl = RateLimiter::new(2, Duration::from_secs(60));
        rl.allow("d");
        rl.allow("e");
        assert_eq!(rl.key_count(), 2);
        rl.clear();
        assert_eq!(rl.key_count(), 0);
    }

    #[test]
    fn test_evict_stale() {
        let rl = RateLimiter::new(5, Duration::from_millis(50));
        rl.allow("f");
        rl.allow("g");
        assert_eq!(rl.key_count(), 2);
        thread::sleep(Duration::from_millis(60));
        rl.evict_stale();
        assert_eq!(rl.key_count(), 0);
    }
}
