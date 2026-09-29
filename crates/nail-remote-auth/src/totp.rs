//! TOTP (RFC 6238: HMAC-SHA1, 6 digits, 30-second steps, ±1 step of clock
//! skew). Secrets live in the OS keychain ([`SecretStore`]); the store
//! keeps only the `keychain:` reference, and every accepted time step is
//! recorded so a code works once (`Store::accept_totp_step`).
//!
//! Enrollment is two-step: [`start_enrollment`] puts a new secret under a
//! *pending* reference and returns it once; [`confirm_enrollment`] checks a
//! code against it and only then moves it to the user's reference.

use nail_api::RemoteUserId;
use nail_api::accounts::SecretRef;
use nail_api::engine::SecretStore;
use nail_api::remote::TotpEnrollment;
use rand::RngCore;
use rand::rngs::OsRng;
use totp_rs::{Algorithm, Secret, Totp};
use zeroize::Zeroizing;

use crate::error::{AuthError, Result};

/// Issuer shown by authenticator apps (a product name, not UI text).
pub const ISSUER: &str = "Nail";
/// Code length.
pub const DIGITS: u8 = 6;
/// Time step in seconds.
pub const PERIOD_SECS: u64 = 30;
/// Steps of clock skew accepted on either side.
pub const SKEW_STEPS: u16 = 1;
/// Secret length in bytes (RFC 4226 recommends 160 bits).
pub const SECRET_BYTES: usize = 20;

/// The keychain reference of a user's confirmed TOTP secret.
pub fn secret_ref(user: &RemoteUserId) -> SecretRef {
    SecretRef(format!(
        "{}{}/remote-totp/{}",
        SecretRef::SCHEME,
        SecretRef::SERVICE,
        user
    ))
}

/// The keychain reference of a secret waiting for confirmation.
pub fn pending_ref(user: &RemoteUserId) -> SecretRef {
    SecretRef(format!(
        "{}{}/remote-totp-pending/{}",
        SecretRef::SCHEME,
        SecretRef::SERVICE,
        user
    ))
}

fn build(secret: &[u8], account_name: &str) -> Result<Totp> {
    // otpauth account names must not contain ':'.
    let account: String = account_name
        .chars()
        .map(|c| if c == ':' || c.is_control() { '_' } else { c })
        .collect();
    let account = if account.trim().is_empty() {
        "remote".to_owned()
    } else {
        account
    };
    totp_rs::Builder::new()
        .with_algorithm(Algorithm::SHA1)
        .with_digits(DIGITS)
        .with_skew(SKEW_STEPS)
        .with_step_duration(PERIOD_SECS)
        .with_secret(secret.to_vec())
        .with_account_name(account)
        .with_issuer(Some(ISSUER))
        .build()
        .map_err(|e| AuthError::Internal(format!("totp: {e}")))
}

fn decode(encoded: &str) -> Result<Zeroizing<Vec<u8>>> {
    let secret = Secret::try_from_base32(encoded.trim())
        .map_err(|_| AuthError::Internal("the stored TOTP secret is not base32".into()))?;
    Ok(Zeroizing::new(secret.as_bytes().to_vec()))
}

/// A code as typed: digits only, spaces and dashes ignored; `None` when it
/// cannot be a code.
fn normalize_code(code: &str) -> Option<String> {
    let digits: String = code
        .chars()
        .filter(|c| !c.is_whitespace() && *c != '-')
        .collect();
    (digits.len() == usize::from(DIGITS) && digits.bytes().all(|b| b.is_ascii_digit()))
        .then_some(digits)
}

/// The time step a code is valid for at `now` (within the skew), `None`
/// for a wrong code. The caller must still accept the step once.
pub fn matching_step(secret: &[u8], code: &str, now: i64) -> Option<i64> {
    let code = normalize_code(code)?;
    let totp = build(secret, "check").ok()?;
    let now = u64::try_from(now).ok()?;
    totp.check(&code, now)
        .and_then(|step| i64::try_from(step).ok())
}

/// The code for `secret` at `time` (tests and the enrollment check).
pub fn code_at(secret: &[u8], time: i64) -> Option<String> {
    let totp = build(secret, "code").ok()?;
    Some(totp.generate(u64::try_from(time).ok()?).to_string())
}

/// Reads a user's confirmed secret from the keychain.
pub fn load_secret(
    secrets: &dyn SecretStore,
    reference: &str,
) -> Result<Option<Zeroizing<Vec<u8>>>> {
    let reference = SecretRef::parse(reference)
        .map_err(|e| AuthError::Internal(format!("bad TOTP reference: {e:?}")))?;
    match secrets.get(&reference)? {
        Some(encoded) => {
            let encoded = Zeroizing::new(encoded);
            Ok(Some(decode(&encoded)?))
        }
        None => Ok(None),
    }
}

/// Makes a new secret for `user`, keeps it under the pending reference and
/// returns what the user scans (shown once).
pub fn start_enrollment(
    secrets: &dyn SecretStore,
    user: &RemoteUserId,
    account_name: &str,
) -> Result<TotpEnrollment> {
    let mut secret = Zeroizing::new([0_u8; SECRET_BYTES]);
    OsRng.fill_bytes(secret.as_mut());
    let totp = build(secret.as_ref(), account_name)?;
    let encoded = Zeroizing::new(Secret::from(secret.to_vec()).to_base32());
    secrets.set(&pending_ref(user), &encoded)?;
    Ok(TotpEnrollment {
        user_id: user.clone(),
        secret: encoded.trim_end_matches('=').to_owned(),
        otpauth_uri: totp
            .to_url()
            .map_err(|e| AuthError::Internal(format!("otpauth: {e}")))?,
        issuer: ISSUER.to_owned(),
        account_name: totp.account_name().to_owned(),
        digits: u32::from(DIGITS),
        period_secs: u32::try_from(PERIOD_SECS).unwrap_or(30),
    })
}

/// Checks `code` against the pending secret at `now`. On success the
/// secret moves to the user's reference, which is returned (store it with
/// `Store::set_remote_totp`) together with the accepted step (record it
/// with `Store::accept_totp_step` so the code cannot be used to log in).
pub fn confirm_enrollment(
    secrets: &dyn SecretStore,
    user: &RemoteUserId,
    code: &str,
    now: i64,
) -> Result<(SecretRef, i64)> {
    let pending = pending_ref(user);
    let encoded = secrets
        .get(&pending)?
        .map(Zeroizing::new)
        .ok_or_else(|| AuthError::Invalid("no TOTP enrollment is waiting".into()))?;
    let secret = decode(&encoded)?;
    let step = matching_step(&secret, code, now)
        .ok_or_else(|| AuthError::Invalid("the TOTP code is wrong".into()))?;
    let reference = secret_ref(user);
    secrets.set(&reference, &encoded)?;
    secrets.delete(&pending)?;
    Ok((reference, step))
}

/// Removes a user's TOTP secrets (confirmed and pending) from the keychain.
pub fn delete_secrets(secrets: &dyn SecretStore, user: &RemoteUserId) -> Result<()> {
    secrets.delete(&secret_ref(user))?;
    secrets.delete(&pending_ref(user))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::engine::MemorySecretStore;

    const RFC_SECRET: &[u8] = b"12345678901234567890";

    #[test]
    fn rfc_6238_vectors_with_six_digits() {
        // RFC 6238 appendix B (SHA-1), last six digits.
        for (time, code) in [
            (59, "287082"),
            (1_111_111_109, "081804"),
            (1_234_567_890, "005924"),
            (2_000_000_000, "279037"),
        ] {
            assert_eq!(code_at(RFC_SECRET, time).unwrap(), code, "t={time}");
        }
    }

    #[test]
    fn skew_of_one_step_either_side() {
        let now = 1_000_000_020; // step 33_333_334
        let code = code_at(RFC_SECRET, now).unwrap();
        let step = now / 30;
        assert_eq!(matching_step(RFC_SECRET, &code, now), Some(step));
        // The server's clock is ahead or behind by up to one step.
        assert_eq!(matching_step(RFC_SECRET, &code, now + 30), Some(step));
        assert_eq!(matching_step(RFC_SECRET, &code, now - 30), Some(step));
        // Two steps is too far.
        assert_eq!(matching_step(RFC_SECRET, &code, now + 60), None);
        assert_eq!(matching_step(RFC_SECRET, &code, now - 60), None);
    }

    #[test]
    fn codes_are_normalized_and_checked_strictly() {
        let now = 1_234_567_890;
        assert!(matching_step(RFC_SECRET, "005 924", now).is_some());
        assert!(matching_step(RFC_SECRET, "005-924", now).is_some());
        assert!(matching_step(RFC_SECRET, "5924", now).is_none());
        assert!(matching_step(RFC_SECRET, "0059245", now).is_none());
        assert!(matching_step(RFC_SECRET, "00592a", now).is_none());
        assert!(matching_step(RFC_SECRET, "", now).is_none());
        assert!(matching_step(RFC_SECRET, "005925", now).is_none());
        assert!(matching_step(RFC_SECRET, "005924", -5).is_none());
    }

    #[test]
    fn enrollment_moves_the_secret_only_after_a_good_code() {
        let secrets = MemorySecretStore::new();
        let user = RemoteUserId::new("ruser-1");
        let enrollment = start_enrollment(&secrets, &user, "ada:lovelace").unwrap();
        assert_eq!(enrollment.digits, 6);
        assert_eq!(enrollment.period_secs, 30);
        assert_eq!(enrollment.issuer, "Nail");
        assert_eq!(enrollment.account_name, "ada_lovelace");
        assert!(
            enrollment
                .otpauth_uri
                .starts_with("otpauth://totp/Nail:ada_lovelace?"),
            "{}",
            enrollment.otpauth_uri
        );
        assert!(!enrollment.secret.contains('='));
        assert!(secrets.get(&secret_ref(&user)).unwrap().is_none());
        assert!(secrets.get(&pending_ref(&user)).unwrap().is_some());

        let secret = Secret::try_from_base32(&enrollment.secret).unwrap();
        let now = 1_800_000_000;
        assert!(
            matches!(
                confirm_enrollment(&secrets, &user, "000000", now),
                Err(AuthError::Invalid(_))
            ) || code_at(secret.as_bytes(), now).as_deref() == Some("000000")
        );
        let code = code_at(secret.as_bytes(), now).unwrap();
        let (reference, step) = confirm_enrollment(&secrets, &user, &code, now).unwrap();
        assert_eq!(reference, secret_ref(&user));
        assert_eq!(step, now / 30);
        assert!(secrets.get(&pending_ref(&user)).unwrap().is_none());
        let loaded = load_secret(&secrets, reference.as_str()).unwrap().unwrap();
        assert_eq!(loaded.as_slice(), secret.as_bytes());
        // Nothing pending any more.
        assert!(matches!(
            confirm_enrollment(&secrets, &user, &code, now),
            Err(AuthError::Invalid(_))
        ));
        delete_secrets(&secrets, &user).unwrap();
        assert!(load_secret(&secrets, reference.as_str()).unwrap().is_none());
    }

    #[test]
    fn references_are_keychain_references() {
        let user = RemoteUserId::new("ruser-7");
        assert_eq!(
            secret_ref(&user).as_str(),
            "keychain:nail/remote-totp/ruser-7"
        );
        assert!(SecretRef::parse(pending_ref(&user).as_str()).is_ok());
    }
}
