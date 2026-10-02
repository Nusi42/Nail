//! The vault recovery key (M4c, ADR 0094).
//!
//! With encryption at rest the data key lives in the OS keychain
//! (ADR 0032). If that entry is lost (a new Windows profile, a reset
//! Credential Manager), the data would be unreadable. A recovery key is a
//! second way in: 32 random bytes the user writes down or saves, from
//! which `nail-vault` derives a key that wraps the data key (stored next to
//! the data, never the recovery key itself).
//!
//! * [`crate::Request::CreateRecoveryKey`] makes one (replacing an older
//!   one) and returns it **once**, formatted ([`format_recovery_key`]);
//!   it can also write a small text file ("recovery kit").
//! * [`crate::Request::VerifyRecoveryKey`] checks a typed key without
//!   changing anything (the UI asks for it right after creating).
//! * [`crate::Request::RestoreVaultKey`] unwraps the data key with a typed
//!   key and puts it back into the keychain — the way out of
//!   [`crate::security::VaultStatus::needs_recovery`].
//!
//! # Text form
//!
//! The 32 bytes are written in Crockford base32 (52 characters, the last
//! carrying 1 data bit and 4 zero bits), followed by 4 check characters
//! (the top 20 bits of the CRC-32 of the bytes), in 14 groups of 4 joined
//! by `-`: `ABCD-EFGH-…`. Parsing ignores case, spaces and dashes and
//! reads `O` as `0` and `I` / `L` as `1`, so a key copied from paper still
//! works; the check characters catch typos before the vault is asked.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// Bytes of a recovery key.
pub const RECOVERY_KEY_BYTES: usize = 32;

/// Characters of the formatted key without dashes.
pub const RECOVERY_KEY_CHARS: usize = 56;

const DATA_CHARS: usize = 52;
const GROUP: usize = 4;
const ALPHABET: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";

/// Whether a recovery key exists and when it was made / last checked
/// (part of [`crate::security::VaultStatus`]).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct RecoveryKeyStatus {
    /// A recovery key wraps the current data key.
    pub configured: bool,
    /// When it was created (RFC 3339).
    pub created_at: Option<String>,
    /// When the user last verified it (RFC 3339).
    pub verified_at: Option<String>,
}

labeled_enum! {
    /// Why a typed recovery key is not even a key (`recovery-key-issue-*`).
    pub enum RecoveryKeyIssue : "recovery-key-issue" {
        /// Nothing typed.
        Empty = "empty",
        /// A character that is not in the alphabet.
        InvalidCharacter = "invalid-character",
        /// Too few or too many characters.
        WrongLength = "wrong-length",
        /// The check characters do not match (a typo).
        ChecksumMismatch = "checksum-mismatch",
    }
}

fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = 0xFFFF_FFFFu32;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

fn push_bits(out: &mut String, bytes: &[u8], chars: usize) {
    let mut acc: u64 = 0;
    let mut bits = 0u32;
    let mut written = 0;
    for &b in bytes {
        acc = (acc << 8) | u64::from(b);
        bits += 8;
        while bits >= 5 && written < chars {
            bits -= 5;
            out.push(ALPHABET[((acc >> bits) & 31) as usize] as char);
            written += 1;
        }
    }
    if bits > 0 && written < chars {
        out.push(ALPHABET[((acc << (5 - bits)) & 31) as usize] as char);
    }
}

/// The text form of a recovery key (see the module docs).
pub fn format_recovery_key(key: &[u8; RECOVERY_KEY_BYTES]) -> String {
    let mut plain = String::with_capacity(RECOVERY_KEY_CHARS);
    push_bits(&mut plain, key, DATA_CHARS);
    let check = crc32(key) >> 12; // 20 bits
    for shift in [15u32, 10, 5, 0] {
        plain.push(ALPHABET[((check >> shift) & 31) as usize] as char);
    }
    let mut out = String::with_capacity(RECOVERY_KEY_CHARS + RECOVERY_KEY_CHARS / GROUP);
    for (i, c) in plain.chars().enumerate() {
        if i > 0 && i % GROUP == 0 {
            out.push('-');
        }
        out.push(c);
    }
    out
}

fn value_of(c: char) -> Option<u8> {
    let c = match c.to_ascii_uppercase() {
        'O' => '0',
        'I' | 'L' => '1',
        c => c,
    };
    ALPHABET
        .iter()
        .position(|&a| a as char == c)
        .map(|p| p as u8)
}

/// Reads a typed recovery key back into its bytes.
pub fn parse_recovery_key(text: &str) -> Result<[u8; RECOVERY_KEY_BYTES], RecoveryKeyIssue> {
    let mut values = Vec::with_capacity(RECOVERY_KEY_CHARS);
    for c in text.chars() {
        if c.is_whitespace() || c == '-' {
            continue;
        }
        values.push(value_of(c).ok_or(RecoveryKeyIssue::InvalidCharacter)?);
    }
    if values.is_empty() {
        return Err(RecoveryKeyIssue::Empty);
    }
    if values.len() != RECOVERY_KEY_CHARS {
        return Err(RecoveryKeyIssue::WrongLength);
    }
    let mut key = [0u8; RECOVERY_KEY_BYTES];
    let mut acc: u64 = 0;
    let mut bits = 0u32;
    let mut index = 0;
    for &v in &values[..DATA_CHARS] {
        acc = (acc << 5) | u64::from(v);
        bits += 5;
        if bits >= 8 && index < RECOVERY_KEY_BYTES {
            bits -= 8;
            key[index] = ((acc >> bits) & 0xFF) as u8;
            index += 1;
        }
    }
    // The 4 padding bits must be zero.
    if acc & ((1 << bits) - 1) != 0 {
        return Err(RecoveryKeyIssue::ChecksumMismatch);
    }
    let check = values[DATA_CHARS..]
        .iter()
        .fold(0u32, |acc, &v| (acc << 5) | u32::from(v));
    if check != crc32(&key) >> 12 {
        return Err(RecoveryKeyIssue::ChecksumMismatch);
    }
    Ok(key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::labels::assert_labels;
    use pretty_assertions::assert_eq;

    fn sample() -> [u8; RECOVERY_KEY_BYTES] {
        let mut key = [0u8; RECOVERY_KEY_BYTES];
        for (i, b) in key.iter_mut().enumerate() {
            *b = (i as u8).wrapping_mul(37).wrapping_add(11);
        }
        key
    }

    #[test]
    fn crc32_matches_the_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
    }

    #[test]
    fn round_trip_and_shape() {
        for key in [sample(), [0u8; 32], [0xFFu8; 32]] {
            let text = format_recovery_key(&key);
            assert_eq!(text.len(), RECOVERY_KEY_CHARS + 13);
            assert_eq!(text.split('-').count(), 14);
            assert!(text.split('-').all(|g| g.len() == 4));
            assert_eq!(parse_recovery_key(&text), Ok(key));
        }
    }

    #[test]
    fn parsing_forgives_case_spaces_and_lookalikes() {
        let key = sample();
        let text = format_recovery_key(&key);
        let sloppy = text.to_lowercase().replace('-', " ");
        assert_eq!(parse_recovery_key(&sloppy), Ok(key));
        let zero = format_recovery_key(&[0u8; 32]).replace('0', "O");
        assert_eq!(parse_recovery_key(&zero), Ok([0u8; 32]));
    }

    #[test]
    fn parsing_catches_mistakes() {
        let text = format_recovery_key(&sample());
        assert_eq!(parse_recovery_key("  "), Err(RecoveryKeyIssue::Empty));
        assert_eq!(
            parse_recovery_key("ABCD-U000"),
            Err(RecoveryKeyIssue::InvalidCharacter)
        );
        assert_eq!(
            parse_recovery_key(&text[..20]),
            Err(RecoveryKeyIssue::WrongLength)
        );
        // Change one data character.
        let mut chars: Vec<char> = text.chars().collect();
        chars[0] = if chars[0] == 'A' { 'B' } else { 'A' };
        let typo: String = chars.into_iter().collect();
        assert_eq!(
            parse_recovery_key(&typo),
            Err(RecoveryKeyIssue::ChecksumMismatch)
        );
        assert_labels(
            RecoveryKeyIssue::ALL,
            RecoveryKeyIssue::as_str,
            RecoveryKeyIssue::message_id,
        );
    }
}
