//! Integer <-> float conversions of the statistics and the synthetic
//! corpus. The values are counts, sizes and seconds far below 2^52, where
//! `f64` is exact; float -> integer conversions are clamped by the callers
//! first and truncate toward zero on purpose (Rust's `as` saturates).

/// A count or size as `f64` (exact below 2^52).
#[expect(
    clippy::cast_precision_loss,
    reason = "counts and sizes here stay far below 2^52, where f64 is exact"
)]
pub fn f64_of(n: u64) -> f64 {
    n as f64
}

/// A length as `f64` (exact below 2^52).
pub fn f64_of_len(n: usize) -> f64 {
    f64_of(u64::try_from(n).unwrap_or(u64::MAX))
}

/// A non-negative float as `usize`, truncated toward zero (saturating;
/// negative and NaN become 0).
#[expect(
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    reason = "truncation toward zero is the intended rounding; `as` saturates out-of-range values"
)]
pub fn usize_of(x: f64) -> usize {
    x as usize
}

/// A float as `i64`, truncated toward zero (saturating).
#[expect(
    clippy::cast_possible_truncation,
    reason = "truncation toward zero is the intended rounding; `as` saturates out-of-range values"
)]
pub fn i64_of(x: f64) -> i64 {
    x as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn conversions_truncate_and_saturate() {
        assert!((f64_of(3) - 3.0).abs() < f64::EPSILON);
        assert!((f64_of_len(7) - 7.0).abs() < f64::EPSILON);
        assert_eq!(usize_of(2.9), 2);
        assert_eq!(usize_of(-1.0), 0);
        assert_eq!(usize_of(f64::NAN), 0);
        assert_eq!(i64_of(-2.9), -2);
        assert_eq!(i64_of(f64::INFINITY), i64::MAX);
    }
}
