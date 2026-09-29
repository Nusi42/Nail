//! One-time pairing codes. A code is created on the desktop (which is the
//! approval of the device that enters it), shown as `XXXX-XXXX` from an
//! alphabet without look-alikes, stored only as a SHA-256 of its
//! normalized form, valid for a few minutes and redeemable once
//! (`Store::redeem_pairing_code`). The pending pairing carries the new
//! device's name and grant, so redeeming it creates exactly that device.

use nail_api::RemoteUserId;
use nail_api::remote::{PairingCode, RemoteGrant};
use nail_store::{NewPairingCode, Store, hash_remote_secret};
use rand::Rng;
use rand::rngs::OsRng;
use zeroize::Zeroizing;

use crate::error::{AuthError, Result};

/// Characters of a code: no `0/O`, `1/I/L` or `U/V` to confuse.
pub const ALPHABET: &[u8] = b"ABCDEFGHJKMNPQRSTWXYZ23456789";

/// Characters of a code (without the separator). 29 distinct symbols over
/// eight positions: about 38.9 bits, against an online guesser limited by
/// the per-address throttle and the code's short life.
pub const CODE_LEN: usize = 8;

/// Default life of a code.
pub const DEFAULT_TTL_MINUTES: u32 = 10;
/// Longest life a code can be given.
pub const MAX_TTL_MINUTES: u32 = 60;

fn symbols() -> &'static [u8] {
    ALPHABET
}

/// A new random code, formatted `XXXX-XXXX`.
pub fn generate_code() -> Zeroizing<String> {
    let symbols = symbols();
    let mut code = Zeroizing::new(String::with_capacity(CODE_LEN + 1));
    for i in 0..CODE_LEN {
        if i == CODE_LEN / 2 {
            code.push('-');
        }
        let index = OsRng.gen_range(0..symbols.len());
        code.push(char::from(symbols[index]));
    }
    code
}

/// The code as typed, normalized: separators and spaces removed, upper
/// case. `None` when it cannot be a code (wrong length, a character
/// outside [`ALPHABET`]).
pub fn normalize(code: &str) -> Option<Zeroizing<String>> {
    let normalized: Zeroizing<String> = Zeroizing::new(
        code.chars()
            .filter(|c| !c.is_whitespace() && *c != '-' && *c != '_')
            .map(|c| c.to_ascii_uppercase())
            .collect(),
    );
    (normalized.len() == CODE_LEN && normalized.bytes().all(|b| symbols().contains(&b)))
        .then_some(normalized)
}

/// The stored form of a code.
pub fn code_hash(normalized: &str) -> String {
    hash_remote_secret(normalized)
}

/// A pairing code just created: the stored record and the code itself
/// (shown once on the desktop).
#[derive(Debug)]
pub struct CreatedPairing {
    /// The stored record.
    pub pairing: PairingCode,
    /// The code, `XXXX-XXXX`.
    pub code: Zeroizing<String>,
}

/// Creates a pending pairing for `user` at `now`: a new device named
/// `device_name` (or what the browser sends) with `grant` (the user's
/// grant when `None`; always narrowed to it), valid for `ttl_minutes`
/// (default 10, at most 60).
pub fn create(
    store: &Store,
    user: &RemoteUserId,
    device_name: Option<&str>,
    grant: Option<&RemoteGrant>,
    ttl_minutes: Option<u32>,
    now: i64,
) -> Result<CreatedPairing> {
    let ttl = ttl_minutes.unwrap_or(DEFAULT_TTL_MINUTES);
    if ttl == 0 || ttl > MAX_TTL_MINUTES {
        return Err(AuthError::Invalid(format!(
            "a pairing code lives 1 to {MAX_TTL_MINUTES} minutes"
        )));
    }
    let owner = store.get_remote_user(user)?;
    if owner.disabled {
        return Err(AuthError::Invalid(format!(
            "remote user {user} is disabled"
        )));
    }
    let device_name = match device_name.map(str::trim) {
        Some(name) if !name.is_empty() => Some(crate::session::clean_label(name)?),
        _ => None,
    };
    let code = generate_code();
    let normalized =
        normalize(&code).ok_or_else(|| AuthError::Internal("generated a bad code".into()))?;
    let pairing = store.create_pairing_code(
        &NewPairingCode {
            user_id: user.clone(),
            code_hash: code_hash(&normalized),
            device_name,
            grant: grant.cloned().unwrap_or_else(|| owner.grant.clone()),
            expires_at: now + i64::from(ttl) * 60,
        },
        now,
    )?;
    Ok(CreatedPairing { pairing, code })
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::remote::{PairingState, RemoteAccessPreset};

    #[test]
    fn codes_use_the_unambiguous_alphabet() {
        for _ in 0..200 {
            let code = generate_code();
            assert_eq!(code.len(), CODE_LEN + 1);
            assert_eq!(&code[4..5], "-");
            for c in code.chars().filter(|c| *c != '-') {
                assert!(!"01ILOUV".contains(c), "{}", code.as_str());
            }
            assert!(normalize(&code).is_some());
        }
        assert_ne!(*generate_code(), *generate_code());
        let distinct: std::collections::BTreeSet<u8> = symbols().iter().copied().collect();
        assert_eq!(distinct.len(), symbols().len());
    }

    #[test]
    fn normalization() {
        assert_eq!(normalize("k7qm-2wxr").unwrap().as_str(), "K7QM2WXR");
        assert_eq!(normalize(" K7QM 2WXR ").unwrap().as_str(), "K7QM2WXR");
        assert!(normalize("K7QM-2WX").is_none());
        assert!(normalize("K7QM-2WXRS").is_none());
        assert!(normalize("K0QM-2WXR").is_none(), "0 is not in the alphabet");
        assert!(normalize("K7QM-2WXÄ").is_none());
        assert_eq!(code_hash("K7QM2WXR"), hash_remote_secret("K7QM2WXR"));
    }

    #[test]
    fn create_stores_only_the_hash_and_redeems_once() {
        let store = Store::open_in_memory().unwrap();
        let user = store
            .create_remote_user("ada", None, &RemoteAccessPreset::NoSend.grant(), 1)
            .unwrap();
        let now = 1_000;
        let created = create(
            &store,
            &user.id,
            Some("  Phone "),
            Some(&RemoteAccessPreset::Full.grant()),
            None,
            now,
        )
        .unwrap();
        assert_eq!(created.pairing.state, PairingState::Pending);
        assert_eq!(created.pairing.device_name.as_deref(), Some("Phone"));
        // The grant is narrowed to the user's.
        assert_eq!(created.pairing.grant, RemoteAccessPreset::NoSend.grant());
        let hash = code_hash(&normalize(&created.code).unwrap());
        let redeemed = store.redeem_pairing_code(&hash, now + 599).unwrap();
        assert_eq!(redeemed.id, created.pairing.id);
        assert!(store.redeem_pairing_code(&hash, now + 599).is_err());

        let late = create(&store, &user.id, None, None, Some(1), now).unwrap();
        let hash = code_hash(&normalize(&late.code).unwrap());
        assert!(store.redeem_pairing_code(&hash, now + 60).is_err());

        assert!(matches!(
            create(&store, &user.id, None, None, Some(0), now),
            Err(AuthError::Invalid(_))
        ));
        assert!(matches!(
            create(&store, &user.id, None, None, Some(61), now),
            Err(AuthError::Invalid(_))
        ));
        assert!(matches!(
            create(
                &store,
                &RemoteUserId::new("ruser-99"),
                None,
                None,
                None,
                now
            ),
            Err(AuthError::Store(_))
        ));
    }
}
