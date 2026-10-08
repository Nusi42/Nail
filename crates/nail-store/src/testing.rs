//! Test support (feature `testing`, on in the crate's own tests): a
//! stand-in cipher for the encryption-at-rest hooks (the real one lives in
//! nail-vault) and the account every store test starts from.

use nail_api::{AccountKind, FolderRole, testing};

use crate::Store;
use crate::seal::{CipherError, DataCipher, SEALED_MAGIC};

/// Adds the IMAP account `account_id` (`nail_api::testing::account`) with
/// its inbox `inbox` (path `INBOX`).
///
/// # Panics
///
/// When the store refuses the writes.
#[expect(
    clippy::expect_used,
    reason = "test support: a failed fixture write ends the test"
)]
pub fn add_inbox(store: &Store, account_id: &str, inbox: &str) {
    store
        .upsert_account(&testing::account(account_id, AccountKind::Imap))
        .expect("account stored");
    store
        .upsert_folder(&testing::folder(
            inbox,
            account_id,
            "INBOX",
            FolderRole::Inbox,
        ))
        .expect("inbox stored");
}

/// A reversible test cipher: magic + context length + context + bytes XOR
/// the key byte. Not encryption: it hides ASCII text from a byte search,
/// which is all the tests look for, and it checks the context.
#[derive(Debug, Clone, Copy)]
pub struct XorCipher(pub u8);

impl DataCipher for XorCipher {
    fn seal(&self, context: &[u8], plain: &[u8]) -> Result<Vec<u8>, CipherError> {
        let len = u8::try_from(context.len())
            .map_err(|_| CipherError::Failed("context too long".into()))?;
        let mut out = SEALED_MAGIC.to_vec();
        out.push(len);
        out.extend_from_slice(context);
        out.extend(plain.iter().map(|b| b ^ self.0));
        Ok(out)
    }

    fn open(&self, context: &[u8], sealed: &[u8]) -> Result<Vec<u8>, CipherError> {
        let rest = sealed
            .strip_prefix(&SEALED_MAGIC)
            .ok_or_else(|| CipherError::Failed("magic".into()))?;
        let (&len, rest) = rest
            .split_first()
            .ok_or_else(|| CipherError::Failed("short".into()))?;
        let (ctx, body) = rest
            .split_at_checked(usize::from(len))
            .ok_or_else(|| CipherError::Failed("short".into()))?;
        if ctx != context {
            return Err(CipherError::Failed("context".into()));
        }
        Ok(body.iter().map(|b| b ^ self.0).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_binds_the_context() {
        let cipher = XorCipher(0x5A);
        let sealed = cipher.seal(b"ctx", b"Hello").unwrap();
        assert!(sealed.starts_with(&SEALED_MAGIC));
        assert!(!sealed.windows(5).any(|w| w == b"Hello"));
        assert_eq!(cipher.open(b"ctx", &sealed).unwrap(), b"Hello");
        assert!(cipher.open(b"other", &sealed).is_err());
        assert!(cipher.open(b"ctx", b"plain").is_err());
    }
}
