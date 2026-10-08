//! sRGB colour math of the WCAG 2.x contrast checks: the one definition of
//! hex parsing, relative luminance and the contrast ratio (wave Q2, SSOT /
//! ODR / DRY). The theme contrast test of nail-config uses it; the UI's port
//! (`ui/src/theme/contrast.ts`) is pinned by nail-api's `appearance`
//! vectors.

/// Red, green and blue channels, 0–255 (fractions from `color-mix`).
pub type Rgb = [f64; 3];

/// `#rgb` / `#rrggbb` (any case) → its channels; anything else → `None`.
#[must_use]
pub fn parse_hex(value: &str) -> Option<Rgb> {
    let digits = value.trim().strip_prefix('#')?;
    if !digits.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    let channel = |s: &str| u8::from_str_radix(s, 16).ok().map(f64::from);
    match digits.len() {
        3 => {
            let mut out = [0.0; 3];
            for (slot, c) in out.iter_mut().zip(digits.chars()) {
                *slot = channel(&format!("{c}{c}"))?;
            }
            Some(out)
        }
        6 => Some([
            channel(digits.get(0..2)?)?,
            channel(digits.get(2..4)?)?,
            channel(digits.get(4..6)?)?,
        ]),
        _ => None,
    }
}

/// WCAG 2.x relative luminance (0 = black, 1 = white), with the sRGB
/// transfer function's threshold 0.04045.
#[must_use]
pub fn relative_luminance(rgb: Rgb) -> f64 {
    let linear = |c: f64| {
        let c = c / 255.0;
        if c <= 0.040_45 {
            c / 12.92
        } else {
            ((c + 0.055) / 1.055).powf(2.4)
        }
    };
    let [r, g, b] = rgb.map(linear);
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// The WCAG contrast ratio of two colours (1–21; order does not matter).
#[must_use]
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_hex_colours() {
        assert_eq!(parse_hex("#fff"), Some([255.0; 3]));
        assert_eq!(parse_hex(" #1A2b3C "), Some([26.0, 43.0, 60.0]));
        for bad in [
            "red",
            "#12345",
            "#ggg",
            "fff",
            "#ffffffff",
            "color-mix(in srgb, #fff 50%, #000)",
            "",
        ] {
            assert_eq!(parse_hex(bad), None, "{bad}");
        }
    }

    #[test]
    fn contrast_ratios() {
        let black = [0.0; 3];
        let white = [255.0; 3];
        assert!((contrast_ratio(black, white) - 21.0).abs() < 1e-9);
        assert!((contrast_ratio(white, white) - 1.0).abs() < 1e-9);
        let grey = [118.0; 3];
        assert!((contrast_ratio(grey, white) - 4.54).abs() < 0.01);
        assert!((contrast_ratio(white, grey) - contrast_ratio(grey, white)).abs() < 1e-12);
        assert!((relative_luminance(white) - 1.0).abs() < 1e-12);
    }
}
