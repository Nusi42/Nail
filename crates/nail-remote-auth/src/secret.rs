//! Random secrets (session tokens, device secrets, challenge ids), their
//! stored form, constant-time comparison and the CSRF token.
//!
//! Tokens are 256-bit random values from the OS generator, base64url
//! without padding. They are kept in [`Zeroizing`] buffers while in memory
//! and stored only as `nail_store::hash_remote_secret` (SHA-256). A plain
//! hash is enough for 256-bit random values; looking a hash up in SQLite is
//! not constant time, but the lookup key is a hash of a secret the attacker
//! does not know, so timing reveals nothing usable.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use hmac::{Hmac, KeyInit, Mac};
use rand::RngCore;
use rand::rngs::OsRng;
use sha2::Sha256;
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

/// Bytes of entropy in a session token or device secret.
pub const TOKEN_BYTES: usize = 32;

/// A fresh random token (base64url, no padding).
pub fn random_token() -> Zeroizing<String> {
    let mut bytes = Zeroizing::new([0_u8; TOKEN_BYTES]);
    OsRng.fill_bytes(bytes.as_mut());
    Zeroizing::new(URL_SAFE_NO_PAD.encode(bytes.as_ref()))
}

/// A fresh random identifier (128 bits, base64url), for challenge ids.
pub fn random_id() -> String {
    let mut bytes = [0_u8; 16];
    OsRng.fill_bytes(&mut bytes);
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Whether a presented token has the shape of one of ours (43 base64url
/// characters). Anything else is rejected before touching the store.
pub fn is_token_shaped(token: &str) -> bool {
    token.len() == 43
        && token
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Constant-time equality of two byte strings (length is not secret).
pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && bool::from(a.ct_eq(b))
}

/// Derives CSRF tokens: `HMAC-SHA-256(key, binding)`, base64url. The key
/// is random per process, so tokens become invalid on restart (the browser
/// asks for `Status` again).
pub struct CsrfKey(Zeroizing<[u8; 32]>);

impl std::fmt::Debug for CsrfKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("CsrfKey(..)")
    }
}

impl CsrfKey {
    /// A random key.
    pub fn random() -> Self {
        let mut key = Zeroizing::new([0_u8; 32]);
        OsRng.fill_bytes(key.as_mut());
        Self(key)
    }

    /// The token for `binding` (e.g. `session:rsess-4`).
    pub fn token(&self, binding: &str) -> String {
        URL_SAFE_NO_PAD.encode(self.mac(binding))
    }

    /// Whether `presented` is the token for `binding` (constant time).
    pub fn verify(&self, binding: &str, presented: &str) -> bool {
        let Ok(presented) = URL_SAFE_NO_PAD.decode(presented.trim()) else {
            return false;
        };
        ct_eq(&self.mac(binding), &presented)
    }

    fn mac(&self, binding: &str) -> Vec<u8> {
        #[expect(
            clippy::expect_used,
            reason = "HMAC-SHA256 accepts keys of any length, so new_from_slice cannot fail"
        )]
        let mut mac = <Hmac<Sha256> as KeyInit>::new_from_slice(self.0.as_ref())
            .expect("HMAC accepts any key length");
        mac.update(b"nail-csrf\0");
        mac.update(binding.as_bytes());
        mac.finalize().into_bytes().to_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokens_are_random_and_shaped() {
        let a = random_token();
        let b = random_token();
        assert_ne!(*a, *b);
        assert!(is_token_shaped(&a), "{}", a.as_str());
        assert!(!is_token_shaped("short"));
        assert!(!is_token_shaped(&format!("{}=", &a[..42])));
        assert_eq!(random_id().len(), 22);
    }

    #[test]
    fn constant_time_eq() {
        assert!(ct_eq(b"abc", b"abc"));
        assert!(!ct_eq(b"abc", b"abx"));
        assert!(!ct_eq(b"abc", b"abcd"));
    }

    #[test]
    fn csrf_tokens_are_bound() {
        let key = CsrfKey::random();
        let token = key.token("session:rsess-1");
        assert!(key.verify("session:rsess-1", &token));
        assert!(!key.verify("session:rsess-2", &token));
        assert!(!key.verify("session:rsess-1", "garbage!"));
        assert!(!key.verify("session:rsess-1", ""));
        // Another process (restart) does not accept the token.
        assert!(!CsrfKey::random().verify("session:rsess-1", &token));
        assert_eq!(format!("{key:?}"), "CsrfKey(..)");
    }
}
