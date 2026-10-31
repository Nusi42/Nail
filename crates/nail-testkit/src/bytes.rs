//! Byte patching for crafted test files (zip archives whose headers
//! disagree, repeated names).

/// Replaces the first `count` occurrences of `from` with `to` (same
/// length), scanning forward; panics when there are fewer.
///
/// In a zip archive written front to back, the first occurrence of an
/// entry's name is its local header and the second its central directory
/// entry.
///
/// # Panics
///
/// When `from` and `to` differ in length or `bytes` holds fewer than
/// `count` occurrences of `from`.
pub fn replace_first(bytes: &mut [u8], from: &[u8], to: &[u8], count: usize) {
    assert_eq!(from.len(), to.len(), "same length only");
    let mut at = 0;
    for done in 0..count {
        let Some(pos) = bytes[at..].windows(from.len()).position(|w| w == from) else {
            panic!("found {done} of {count} occurrences");
        };
        let start = at + pos;
        bytes[start..start + to.len()].copy_from_slice(to);
        at = start + to.len();
    }
}

#[cfg(test)]
mod tests {
    use super::replace_first;

    #[test]
    fn replaces_the_first_occurrences_only() {
        let mut bytes = b"abXabXab".to_vec();
        replace_first(&mut bytes, b"ab", b"cd", 2);
        assert_eq!(bytes, b"cdXcdXab");
    }

    #[test]
    #[should_panic(expected = "found 1 of 2")]
    fn too_few_occurrences_panic() {
        replace_first(&mut b"abX".to_vec(), b"ab", b"cd", 2);
    }
}
