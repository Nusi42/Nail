//! Per-client rate limits (M4a, ADR 0079; design.md: "Rate limits per
//! client"): calls per minute, write tool calls per minute and messages
//! sent per hour, from [`McpClient::rate_limits`] (read fresh on every
//! call, so a change applies at once).
//!
//! Sliding windows in memory: a restart forgets them, which only ever
//! lets a client call again sooner.

use std::collections::{HashMap, VecDeque};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use nail_api::McpClientId;
use nail_api::mcp::McpClient;

/// Which limit.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Window {
    /// Calls of any kind per minute.
    Requests,
    /// Write tool calls per minute.
    Writes,
    /// Messages sent per hour.
    Sends,
}

impl Window {
    /// The limit's name in [`nail_api::mcp::McpRateLimits`].
    pub fn name(self) -> &'static str {
        match self {
            Self::Requests => "requests_per_minute",
            Self::Writes => "writes_per_minute",
            Self::Sends => "sends_per_hour",
        }
    }

    fn span(self) -> Duration {
        match self {
            Self::Requests | Self::Writes => Duration::from_mins(1),
            Self::Sends => Duration::from_hours(1),
        }
    }

    fn max(self, client: &McpClient) -> Option<u32> {
        let limits = &client.rate_limits;
        match self {
            Self::Requests => limits.requests_per_minute,
            Self::Writes => limits.writes_per_minute,
            Self::Sends => limits.sends_per_hour,
        }
    }
}

/// A call over a limit.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Limited {
    /// Which limit.
    pub window: Window,
    /// Its value.
    pub max: u32,
    /// When the client may try again (seconds, at least 1).
    pub retry_after_secs: u64,
}

impl std::fmt::Display for Limited {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "rate limit `{}` ({}) reached; retry after {} s",
            self.window.name(),
            self.max,
            self.retry_after_secs
        )
    }
}

/// The windows of every client.
#[derive(Debug, Default)]
pub struct RateLimiter {
    windows: Mutex<HashMap<(McpClientId, Window), VecDeque<Instant>>>,
}

impl RateLimiter {
    /// Counts `count` uses of `window` for `client` now, or refuses them
    /// (counting nothing) when they would go over its limit.
    pub fn check(&self, client: &McpClient, window: Window, count: u32) -> Result<(), Limited> {
        self.check_at(client, window, count, Instant::now())
    }

    fn check_at(
        &self,
        client: &McpClient,
        window: Window,
        count: u32,
        at: Instant,
    ) -> Result<(), Limited> {
        let Some(max) = window.max(client) else {
            return Ok(());
        };
        let mut windows = self
            .windows
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let uses = windows.entry((client.id.clone(), window)).or_default();
        let span = window.span();
        while uses
            .front()
            .is_some_and(|t| at.saturating_duration_since(*t) >= span)
        {
            uses.pop_front();
        }
        let in_window = u32::try_from(uses.len()).unwrap_or(u32::MAX);
        if in_window.saturating_add(count) > max {
            // The oldest use that has to leave the window first.
            let leaving = (in_window.saturating_add(count) - max) as usize;
            let retry = uses.get(leaving.saturating_sub(1)).map_or(span, |t| {
                span.saturating_sub(at.saturating_duration_since(*t))
            });
            return Err(Limited {
                window,
                max,
                retry_after_secs: retry.as_secs().max(1),
            });
        }
        uses.extend(std::iter::repeat_n(at, count as usize));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::mcp::McpRateLimits;

    fn client(limits: McpRateLimits) -> McpClient {
        McpClient {
            id: "1".into(),
            name: "Claude".into(),
            created_at: String::new(),
            last_used_at: None,
            token_hint: String::new(),
            grant: nail_api::mcp::McpGrant::default(),
            revoked_at: None,
            rate_limits: limits,
        }
    }

    #[test]
    fn windows_fill_and_slide() {
        let limiter = RateLimiter::default();
        let c = client(McpRateLimits {
            requests_per_minute: Some(2),
            writes_per_minute: None,
            sends_per_hour: Some(1),
        });
        let t0 = Instant::now();
        assert!(limiter.check_at(&c, Window::Requests, 1, t0).is_ok());
        assert!(limiter.check_at(&c, Window::Requests, 1, t0).is_ok());
        let refusal = limiter
            .check_at(&c, Window::Requests, 1, t0 + Duration::from_secs(20))
            .unwrap_err();
        assert_eq!(refusal.window, Window::Requests);
        assert_eq!(refusal.max, 2);
        assert_eq!(refusal.retry_after_secs, 40);
        assert!(refusal.to_string().contains("requests_per_minute"));
        // A minute later the window has room again.
        assert!(
            limiter
                .check_at(&c, Window::Requests, 1, t0 + Duration::from_secs(61))
                .is_ok()
        );
        // No limit: always fine.
        for _ in 0..100 {
            assert!(limiter.check_at(&c, Window::Writes, 1, t0).is_ok());
        }
        // Sends per hour; a refused batch counts nothing.
        assert!(limiter.check_at(&c, Window::Sends, 2, t0).is_err());
        assert!(limiter.check_at(&c, Window::Sends, 1, t0).is_ok());
        let refusal = limiter.check_at(&c, Window::Sends, 1, t0).unwrap_err();
        assert_eq!(refusal.retry_after_secs, 3600);
    }

    #[test]
    fn clients_are_counted_apart() {
        let limiter = RateLimiter::default();
        let one = client(McpRateLimits {
            writes_per_minute: Some(1),
            ..McpRateLimits::default()
        });
        let mut two = one.clone();
        two.id = "2".into();
        assert!(limiter.check(&one, Window::Writes, 1).is_ok());
        assert!(limiter.check(&one, Window::Writes, 1).is_err());
        assert!(limiter.check(&two, Window::Writes, 1).is_ok());
    }
}
