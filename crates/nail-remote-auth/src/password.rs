//! Passwords: Argon2id PHC strings (OWASP parameters by default), a
//! length policy, and verification that costs the same whether or not the
//! user exists.

use std::sync::OnceLock;

use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::{Algorithm, Argon2, Params, Version};
use zeroize::Zeroizing;

use crate::error::{AuthError, Result};

/// Argon2id cost parameters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Argon2Cost {
    /// Memory in KiB.
    pub memory_kib: u32,
    /// Iterations.
    pub iterations: u32,
    /// Lanes.
    pub parallelism: u32,
}

impl Default for Argon2Cost {
    /// OWASP's recommendation: 19 MiB, 2 iterations, 1 lane.
    fn default() -> Self {
        Self {
            memory_kib: Params::DEFAULT_M_COST,
            iterations: Params::DEFAULT_T_COST,
            parallelism: Params::DEFAULT_P_COST,
        }
    }
}

/// What a new password must look like.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PasswordPolicy {
    /// Minimum length in characters (default
    /// [`nail_api::remote::MIN_PASSWORD_CHARS`]).
    pub min_chars: usize,
    /// Maximum length in bytes (bounds the hashing work).
    pub max_bytes: usize,
    /// Hashing cost for new passwords.
    pub cost: Argon2Cost,
}

impl Default for PasswordPolicy {
    fn default() -> Self {
        Self {
            min_chars: nail_api::remote::MIN_PASSWORD_CHARS,
            max_bytes: 1024,
            cost: Argon2Cost::default(),
        }
    }
}

/// Why a new password is refused.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PasswordIssue {
    /// Shorter than [`PasswordPolicy::min_chars`].
    TooShort,
    /// Longer than [`PasswordPolicy::max_bytes`].
    TooLong,
    /// Only whitespace, or control characters.
    Unusable,
}

impl PasswordPolicy {
    /// Checks a new password against the policy.
    pub fn check(&self, password: &str) -> Result<(), PasswordIssue> {
        if password.len() > self.max_bytes {
            return Err(PasswordIssue::TooLong);
        }
        if password.trim().is_empty() || password.chars().any(char::is_control) {
            return Err(PasswordIssue::Unusable);
        }
        if password.chars().count() < self.min_chars {
            return Err(PasswordIssue::TooShort);
        }
        Ok(())
    }

    fn hasher(&self) -> Result<Argon2<'static>> {
        let params = Params::new(
            self.cost.memory_kib,
            self.cost.iterations,
            self.cost.parallelism,
            None,
        )
        .map_err(|e| AuthError::Internal(format!("argon2 parameters: {e}")))?;
        Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
    }

    /// Hashes a new password (after [`PasswordPolicy::check`]) into an
    /// Argon2id PHC string with a random salt. CPU-heavy: call it from a
    /// blocking task.
    pub fn hash(&self, password: &str) -> Result<String> {
        self.check(password).map_err(|issue| {
            AuthError::Invalid(format!("the password is not acceptable: {issue:?}"))
        })?;
        let hash = self
            .hasher()?
            .hash_password(password.as_bytes())
            .map_err(|e| AuthError::Internal(format!("argon2: {e}")))?;
        Ok(hash.to_string())
    }
}

/// Verifies `password` against a stored PHC string, or — when there is
/// none (unknown user, no password) — against a dummy hash of the same
/// cost so both cases take the same time. Returns `true` only for a
/// stored hash that matches. CPU-heavy: call it from a blocking task.
pub fn verify(password: &str, stored: Option<&str>, cost: Argon2Cost) -> bool {
    let password = Zeroizing::new(password.as_bytes().to_vec());
    match stored {
        Some(hash) if hash.starts_with("$argon2id$") => {
            Argon2::default().verify_password(&password, hash).is_ok()
        }
        _ => {
            let dummy = dummy_hash(cost);
            let _ = Argon2::default().verify_password(&password, dummy.as_str());
            false
        }
    }
}

/// A hash of a random password, made once per process with the first
/// cost asked for (for [`verify`]).
fn dummy_hash(cost: Argon2Cost) -> &'static String {
    static DUMMY: OnceLock<String> = OnceLock::new();
    DUMMY.get_or_init(|| {
        let policy = PasswordPolicy {
            cost,
            ..PasswordPolicy::default()
        };
        policy
            .hasher()
            .ok()
            .and_then(|h| {
                h.hash_password(crate::secret::random_token().as_bytes())
                    .ok()
            })
            .map(|h| h.to_string())
            .unwrap_or_default()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fast() -> PasswordPolicy {
        PasswordPolicy {
            cost: Argon2Cost {
                memory_kib: 1024,
                iterations: 1,
                parallelism: 1,
            },
            ..PasswordPolicy::default()
        }
    }

    #[test]
    fn default_cost_is_owasp() {
        let cost = Argon2Cost::default();
        assert_eq!(
            (cost.memory_kib, cost.iterations, cost.parallelism),
            (19 * 1024, 2, 1)
        );
    }

    #[test]
    fn policy() {
        let policy = PasswordPolicy::default();
        assert_eq!(policy.check("short"), Err(PasswordIssue::TooShort));
        assert_eq!(policy.check("            "), Err(PasswordIssue::Unusable));
        assert_eq!(
            policy.check("tab\tinside password"),
            Err(PasswordIssue::Unusable)
        );
        assert_eq!(policy.check(&"x".repeat(1025)), Err(PasswordIssue::TooLong));
        assert_eq!(policy.check("correct horse battery"), Ok(()));
        // Characters, not bytes: 12 umlauts are enough.
        assert_eq!(policy.check(&"ä".repeat(12)), Ok(()));
    }

    #[test]
    fn hash_and_verify() {
        let policy = fast();
        let hash = policy.hash("correct horse battery").unwrap();
        assert!(hash.starts_with("$argon2id$v=19$m=1024,t=1,p=1$"), "{hash}");
        assert!(verify("correct horse battery", Some(&hash), policy.cost));
        assert!(!verify("correct horse batterY", Some(&hash), policy.cost));
        assert!(!verify("", Some(&hash), policy.cost));
        // No stored hash, or one that is not Argon2id: always false.
        assert!(!verify("correct horse battery", None, policy.cost));
        assert!(!verify(
            "x",
            Some("$argon2i$v=19$m=8,t=1,p=1$c2FsdA$aGFzaA"),
            policy.cost
        ));
        // Two hashes of one password differ (random salt).
        assert_ne!(hash, policy.hash("correct horse battery").unwrap());
        assert!(matches!(policy.hash("short"), Err(AuthError::Invalid(_))));
    }
}
