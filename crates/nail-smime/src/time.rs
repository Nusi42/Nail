//! Times: Unix seconds inside the crate (as the store keeps them), RFC 3339
//! strings on the protocol.

/// Now, Unix seconds.
pub fn now() -> i64 {
    chrono::Utc::now().timestamp()
}

/// RFC 3339 (UTC, seconds precision, `Z`).
pub fn rfc3339(unix: i64) -> String {
    chrono::DateTime::from_timestamp(unix, 0)
        .unwrap_or_default()
        .to_rfc3339_opts(chrono::SecondsFormat::Secs, true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_utc() {
        assert_eq!(rfc3339(0), "1970-01-01T00:00:00Z");
        assert_eq!(rfc3339(4_891_363_200), "2125-01-01T00:00:00Z");
        assert!(now() > 1_700_000_000);
    }
}
