//! Small protocol text forms the clients and the mock servers both write
//! (no dependencies): one definition, so a mock never needs its client.

use std::fmt::Write as _;

/// Ascending numbers as an IMAP sequence set (RFC 9051 §9
/// `sequence-set`): runs collapse to ranges, `1:3,7,9:10`.
#[must_use]
pub fn sequence_set(sorted: &[u32]) -> String {
    let mut out = String::new();
    let mut iter = sorted.iter().copied().peekable();
    while let Some(start) = iter.next() {
        let mut end = start;
        while iter.peek() == Some(&end.wrapping_add(1)) {
            end = iter.next().unwrap_or(end);
        }
        if !out.is_empty() {
            out.push(',');
        }
        if start == end {
            out.push_str(&start.to_string());
        } else {
            let _ = write!(out, "{start}:{end}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sets_collapse_runs() {
        assert_eq!(sequence_set(&[1, 2, 3, 7, 9, 10]), "1:3,7,9:10");
        assert_eq!(sequence_set(&[5]), "5");
        assert_eq!(sequence_set(&[]), "");
        assert_eq!(
            sequence_set(&[u32::MAX - 1, u32::MAX]),
            format!("{}:{}", u32::MAX - 1, u32::MAX)
        );
    }
}
