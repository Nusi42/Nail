//! Brute-force protection: failed attempts are counted per client address
//! (`ip:…`, IPv6 by /64), per login name (`user:…`) and, for pairing codes,
//! globally (`global:pairing`, so guessing from many addresses is slowed
//! down too). The counters live in the store (`remote_throttle`), so a
//! restart does not reset them.
//!
//! * **Exponential backoff** (address and account keys): after
//!   [`ThrottlePolicy::free_failures`] failures within the window, the next
//!   attempt must wait `base · 2^(n − free − 1)` seconds (capped); earlier
//!   attempts are refused with `rate-limited` without being evaluated.
//! * **Lockout:** at the per-kind threshold the key is locked out for
//!   [`ThrottlePolicy::lockout_secs`], doubling with every further failure
//!   in the window (up to [`ThrottlePolicy::max_lockout_secs`]); the caller
//!   raises the lockout alert and writes the audit entry. The global
//!   pairing key only pauses briefly (no backoff, no escalation), and its
//!   count has a short window of its own
//!   ([`ThrottlePolicy::global_window_secs`]), so an attacker cannot keep
//!   pairing shut with a trickle of wrong codes: it takes
//!   [`ThrottlePolicy::global_lockout_failures`] failures within that
//!   window, from addresses that each hit their own backoff and lockout.
//!
//! The address and account window is long (a week) on purpose: with a
//! short window, waiting out the backoff would also age the count out and
//! the lockout would never be reached. A success clears the address's and
//! account's counts (and a successful pairing the global pairing count).

use std::net::IpAddr;

use nail_store::Store;

use crate::error::Result;

/// Limits of the brute-force protection.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ThrottlePolicy {
    /// Failures older than this start a new count (address and account
    /// keys).
    pub window_secs: i64,
    /// The same for the global pairing key: short, so a slow stream of
    /// failures cannot keep pairing paused.
    pub global_window_secs: i64,
    /// Failures within the window before any delay applies.
    pub free_failures: u32,
    /// First delay after the free failures.
    pub base_delay_secs: i64,
    /// Longest delay between attempts.
    pub max_delay_secs: i64,
    /// Failures of one login name that lock it out.
    pub account_lockout_failures: u32,
    /// Failures from one address that lock it out.
    pub ip_lockout_failures: u32,
    /// Failed pairing attempts from all addresses that pause pairing.
    pub global_lockout_failures: u32,
    /// How long an account or address stays locked out at first.
    pub lockout_secs: i64,
    /// Longest lockout (the escalation stops there).
    pub max_lockout_secs: i64,
    /// How long pairing pauses after the global threshold.
    pub global_lockout_secs: i64,
}

impl Default for ThrottlePolicy {
    fn default() -> Self {
        Self {
            window_secs: 7 * 24 * 60 * 60,
            global_window_secs: 60 * 60,
            free_failures: 3,
            base_delay_secs: 1,
            max_delay_secs: 5 * 60,
            account_lockout_failures: 10,
            ip_lockout_failures: 30,
            global_lockout_failures: 100,
            lockout_secs: 15 * 60,
            max_lockout_secs: 24 * 60 * 60,
            global_lockout_secs: 60,
        }
    }
}

/// The key of global pairing failures.
pub const GLOBAL_PAIRING_KEY: &str = "global:pairing";

/// The throttle key of a client address. IPv6 addresses count per /64
/// (one subscriber usually gets a whole /64); IPv4-mapped IPv6 addresses
/// count as their IPv4 address.
pub fn ip_key(ip: Option<IpAddr>) -> String {
    match ip {
        None => "ip:unknown".to_owned(),
        Some(IpAddr::V4(v4)) => format!("ip:{v4}"),
        Some(IpAddr::V6(v6)) => match v6.to_ipv4_mapped() {
            Some(v4) => format!("ip:{v4}"),
            None => {
                let s = v6.segments();
                format!("ip:{:x}:{:x}:{:x}:{:x}::/64", s[0], s[1], s[2], s[3])
            }
        },
    }
}

/// The throttle key of a login name (the same key the core clears when a
/// user is unlocked on the desktop).
pub fn user_key(name: &str) -> String {
    format!("user:{}", name.trim().to_lowercase())
}

/// Whether an attempt may be evaluated now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Gate {
    /// Go ahead.
    Open,
    /// Refused: try again in `retry_after_secs`; `locked` when a lockout
    /// (not just the backoff) is the reason.
    Wait {
        /// Seconds until the next attempt is evaluated.
        retry_after_secs: u32,
        /// A lockout, not a backoff delay.
        locked: bool,
    },
}

impl Gate {
    fn worst(self, other: Gate) -> Gate {
        match (self, other) {
            (Gate::Open, g) | (g, Gate::Open) => g,
            (
                Gate::Wait {
                    retry_after_secs: a,
                    locked: la,
                },
                Gate::Wait {
                    retry_after_secs: b,
                    locked: lb,
                },
            ) => Gate::Wait {
                retry_after_secs: a.max(b),
                locked: la || lb,
            },
        }
    }
}

/// A lockout that started with a recorded failure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lockout {
    /// The locked key (`ip:…`, `user:…`, `global:…`).
    pub key: String,
    /// Locked until (Unix seconds).
    pub until: i64,
}

impl ThrottlePolicy {
    /// Seconds to wait after `failures` failures in the window.
    pub fn delay_after(&self, failures: u32) -> i64 {
        if failures <= self.free_failures {
            return 0;
        }
        let exponent = (failures - self.free_failures - 1).min(30);
        self.base_delay_secs
            .saturating_mul(1_i64 << exponent)
            .min(self.max_delay_secs)
    }

    /// The lockout after `failures` failures of `key`, if any: its length.
    fn lockout_for(&self, key: &str, failures: u32) -> Option<i64> {
        if key.starts_with("global:") {
            return (failures >= self.global_lockout_failures).then_some(self.global_lockout_secs);
        }
        let threshold = if key.starts_with("user:") {
            self.account_lockout_failures
        } else {
            self.ip_lockout_failures
        };
        let beyond = failures.checked_sub(threshold)?.min(30);
        Some(
            self.lockout_secs
                .saturating_mul(1_i64 << beyond)
                .min(self.max_lockout_secs),
        )
    }

    /// Whether an attempt touching `keys` may be evaluated at `now`.
    pub fn check(&self, store: &Store, keys: &[String], now: i64) -> Result<Gate> {
        let mut gate = Gate::Open;
        for key in keys {
            let Some(record) = store.auth_throttle(key)? else {
                continue;
            };
            if let Some(until) = record.locked_until.filter(|until| *until > now) {
                gate = gate.worst(Gate::Wait {
                    retry_after_secs: secs(until - now),
                    locked: true,
                });
                continue;
            }
            if record.first_failure_at <= now - self.window_secs || key.starts_with("global:") {
                continue;
            }
            let ready_at = record.last_failure_at + self.delay_after(record.failures);
            if ready_at > now {
                gate = gate.worst(Gate::Wait {
                    retry_after_secs: secs(ready_at - now),
                    locked: false,
                });
            }
        }
        Ok(gate)
    }

    /// Counts a failed attempt for every key at `now`; returns the
    /// lockouts it started (at most one per key and lockout period).
    pub fn record_failure(&self, store: &Store, keys: &[String], now: i64) -> Result<Vec<Lockout>> {
        let mut lockouts = Vec::new();
        for key in keys {
            let window = if key.starts_with("global:") {
                self.global_window_secs
            } else {
                self.window_secs
            };
            let record = store.record_auth_failure(key, now, window)?;
            let already = record.locked_until.is_some_and(|until| until > now);
            if let Some(duration) = self.lockout_for(key, record.failures)
                && !already
            {
                let until = now + duration;
                store.lock_auth_throttle(key, until)?;
                lockouts.push(Lockout {
                    key: key.clone(),
                    until,
                });
            }
        }
        Ok(lockouts)
    }

    /// Forgets the counters of `keys` (after a success).
    pub fn clear(&self, store: &Store, keys: &[String]) -> Result<()> {
        for key in keys {
            store.clear_auth_throttle(key)?;
        }
        Ok(())
    }
}

fn secs(delta: i64) -> u32 {
    u32::try_from(delta.max(1)).unwrap_or(u32::MAX)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys() {
        assert_eq!(ip_key(Some("192.0.2.7".parse().unwrap())), "ip:192.0.2.7");
        assert_eq!(
            ip_key(Some("::ffff:192.0.2.7".parse().unwrap())),
            "ip:192.0.2.7"
        );
        // Every address of one /64 shares a key.
        assert_eq!(
            ip_key(Some("2001:db8:1:2:aaaa::1".parse().unwrap())),
            ip_key(Some("2001:db8:1:2:ffff::9".parse().unwrap()))
        );
        assert_eq!(
            ip_key(Some("2001:db8:1:2::1".parse().unwrap())),
            "ip:2001:db8:1:2::/64"
        );
        assert_eq!(ip_key(None), "ip:unknown");
        assert_eq!(user_key("  Ada "), "user:ada");
    }

    #[test]
    fn backoff_doubles_up_to_the_cap() {
        let policy = ThrottlePolicy::default();
        let delays: Vec<i64> = (0..=14).map(|n| policy.delay_after(n)).collect();
        assert_eq!(
            delays,
            [0, 0, 0, 0, 1, 2, 4, 8, 16, 32, 64, 128, 256, 300, 300]
        );
        assert_eq!(policy.delay_after(u32::MAX), 300);
    }

    #[test]
    fn failures_back_off_then_lock_out() {
        let store = Store::open_in_memory().unwrap();
        let policy = ThrottlePolicy::default();
        let keys = [user_key("ada"), ip_key(Some("192.0.2.1".parse().unwrap()))];
        let mut now = 10_000;
        for _ in 0..3 {
            assert_eq!(policy.check(&store, &keys, now).unwrap(), Gate::Open);
            assert!(
                policy
                    .record_failure(&store, &keys, now)
                    .unwrap()
                    .is_empty()
            );
        }
        // The fourth failure makes the next attempt wait one second.
        assert!(
            policy
                .record_failure(&store, &keys, now)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            policy.check(&store, &keys, now).unwrap(),
            Gate::Wait {
                retry_after_secs: 1,
                locked: false
            }
        );
        now += 1;
        assert_eq!(policy.check(&store, &keys, now).unwrap(), Gate::Open);
        // Keep failing, always waiting the backoff out.
        let mut lockouts = Vec::new();
        for n in 5..=10 {
            lockouts = policy.record_failure(&store, &keys, now).unwrap();
            now += policy.delay_after(n);
        }
        // The account locks at 10 failures; the address (30) does not.
        assert_eq!(lockouts.len(), 1);
        assert_eq!(lockouts[0].key, "user:ada");
        let gate = policy.check(&store, &keys, now).unwrap();
        assert!(matches!(gate, Gate::Wait { locked: true, .. }), "{gate:?}");
        // Another name from the same address is not locked (but the
        // address's own backoff still applies).
        let other = [user_key("bob"), keys[1].clone()];
        let gate = policy.check(&store, &other, now).unwrap();
        assert!(!matches!(gate, Gate::Wait { locked: true, .. }), "{gate:?}");
        // After the lockout (and the window) the account is open again.
        now = lockouts[0].until + policy.window_secs;
        assert_eq!(policy.check(&store, &keys, now).unwrap(), Gate::Open);
        // A new window starts from one failure.
        policy.record_failure(&store, &keys, now).unwrap();
        assert_eq!(
            store.auth_throttle("user:ada").unwrap().unwrap().failures,
            1
        );
        policy.clear(&store, &keys).unwrap();
        assert!(store.auth_throttle("user:ada").unwrap().is_none());
    }

    #[test]
    fn lockouts_escalate_and_are_capped() {
        let store = Store::open_in_memory().unwrap();
        let policy = ThrottlePolicy::default();
        let keys = [user_key("ada")];
        let mut now = 1_000;
        let mut lengths = Vec::new();
        for _ in 0..18 {
            // Always wait until the key is open again.
            if let Gate::Wait {
                retry_after_secs, ..
            } = policy.check(&store, &keys, now).unwrap()
            {
                now += i64::from(retry_after_secs);
            }
            for lockout in policy.record_failure(&store, &keys, now).unwrap() {
                lengths.push(lockout.until - now);
            }
        }
        assert_eq!(lengths[..4], [900, 1800, 3600, 7200]);
        assert_eq!(*lengths.last().unwrap(), policy.max_lockout_secs);
        // The count never aged out while the attacker waited.
        assert!(store.auth_throttle("user:ada").unwrap().unwrap().failures > 10);
    }

    #[test]
    fn old_failures_expire_with_the_window() {
        let store = Store::open_in_memory().unwrap();
        let policy = ThrottlePolicy::default();
        let keys = [ip_key(None)];
        for _ in 0..8 {
            policy.record_failure(&store, &keys, 100).unwrap();
        }
        assert!(matches!(
            policy.check(&store, &keys, 101).unwrap(),
            Gate::Wait { .. }
        ));
        assert_eq!(
            policy
                .check(&store, &keys, 100 + policy.window_secs)
                .unwrap(),
            Gate::Open
        );
    }

    #[test]
    fn global_pairing_pause_is_short() {
        let store = Store::open_in_memory().unwrap();
        let policy = ThrottlePolicy {
            global_lockout_failures: 3,
            ..ThrottlePolicy::default()
        };
        let keys = [GLOBAL_PAIRING_KEY.to_owned()];
        let mut lockouts = Vec::new();
        for _ in 0..3 {
            lockouts = policy.record_failure(&store, &keys, 500).unwrap();
        }
        assert_eq!(
            lockouts,
            [Lockout {
                key: GLOBAL_PAIRING_KEY.into(),
                until: 560
            }]
        );
        // A lockout is reported once, not again for each further failure.
        assert!(
            policy
                .record_failure(&store, &keys, 501)
                .unwrap()
                .is_empty()
        );
    }

    /// One wrong code a minute cannot keep pairing paused: the global
    /// count starts over after its short window, so the pause comes back
    /// only after the threshold is reached again within one window.
    #[test]
    fn a_trickle_of_failures_does_not_keep_pairing_paused() {
        let store = Store::open_in_memory().unwrap();
        let policy = ThrottlePolicy::default();
        let keys = [GLOBAL_PAIRING_KEY.to_owned()];
        let minute = 60;
        let mut locked_minutes = 0;
        let hours = 6;
        for i in 0..(hours * 60) {
            let now = 1_000 + i * minute;
            policy.record_failure(&store, &keys, now).unwrap();
            // The owner tries half a minute later.
            if policy.check(&store, &keys, now + 30).unwrap() != Gate::Open {
                locked_minutes += 1;
            }
        }
        // 60 failures an hour never reach the threshold of 100.
        assert_eq!(locked_minutes, 0);
        assert!(policy.global_window_secs < policy.window_secs);
    }
}
