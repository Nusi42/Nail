//! Disguised senders (ADR 0195): a `From` whose domain is made of
//! look-alike characters, or whose display name is.
//!
//! The domain is flagged when the link check flags it as a host (mixed
//! scripts, confusable characters; [`crate::links`]), or when its
//! top-level domain is an internationalized one that folds to ASCII
//! (`ċom` → `com`): real top-level domains with accents on Latin letters do
//! not exist. The display name is flagged when it contains invisible
//! characters (zero-width, bidi controls) or letters and digits written with
//! look-alike forms (`ＮＯＶＡＢＡＮＫ`, `𝐍𝐎𝐕𝐀𝐁𝐀𝐍𝐊`) — except next to Chinese,
//! Japanese or Korean text, where full-width Latin is ordinary typography.

use nail_api::Address;
use nail_api::security::MessageWarning;
use unicode_normalization::UnicodeNormalization;
use unicode_normalization::char::is_combining_mark;

use crate::links::lookalike;

/// Warnings for the senders of a message (one per disguised address).
pub fn sender_warnings(from: &[Address]) -> Vec<MessageWarning> {
    from.iter().filter_map(sender_warning).collect()
}

fn sender_warning(sender: &Address) -> Option<MessageWarning> {
    let domain = sender.email.rsplit_once('@').map(|(_, d)| d);
    let disguised = domain.and_then(disguised_domain);
    let name = sender.name.as_deref().and_then(disguised_name);
    if disguised.is_none() && name.is_none() {
        return None;
    }
    let (unicode_domain, resembles) = match disguised {
        Some((unicode, resembles)) => (Some(unicode), resembles),
        None => (None, None),
    };
    Some(MessageWarning::LookalikeSender {
        address: sender.email.clone(),
        unicode_domain,
        resembles,
        name,
    })
}

/// `Some((unicode, resembles))` when an address's domain is disguised.
fn disguised_domain(domain: &str) -> Option<(String, Option<String>)> {
    let Ok(ascii) = idna::domain_to_ascii(domain) else {
        // Not even a valid domain name.
        return Some((domain.to_owned(), None));
    };
    if let Some(found) = lookalike(&ascii) {
        return Some(found);
    }
    let (unicode, result) = idna::domain_to_unicode(&ascii);
    let tld = unicode.rsplit('.').next().unwrap_or_default();
    if result.is_err() || tld.is_ascii() || !fold(tld).is_ascii() {
        return None;
    }
    let folded: String = unicode.split('.').map(fold).collect::<Vec<_>>().join(".");
    let resembles = folded.is_ascii().then_some(folded);
    Some((unicode, resembles))
}

/// Latin letters without their accents (`ș` → `s`, `ċ` → `c`).
fn fold(label: &str) -> String {
    label.nfd().filter(|c| !is_combining_mark(*c)).collect()
}

/// The name as it reads when it is disguised: look-alike letters and digits
/// folded (`Ｓ` → `S`), invisible characters removed.
fn disguised_name(name: &str) -> Option<String> {
    let invisible = name.chars().any(is_invisible);
    let lookalike = !name.chars().any(is_cjk) && name.chars().any(is_lookalike_alphanumeric);
    (invisible || lookalike).then(|| {
        name.chars()
            .filter(|c| !is_invisible(*c))
            .flat_map(|c| {
                let folded: Vec<char> = if is_lookalike_alphanumeric(c) {
                    c.nfkc().collect()
                } else {
                    vec![c]
                };
                folded
            })
            .collect()
    })
}

/// A letter or digit from a block of look-alike forms (full-width,
/// mathematical, circled, squared) that stands for one ASCII letter or digit.
fn is_lookalike_alphanumeric(c: char) -> bool {
    matches!(c,
        '\u{FF10}'..='\u{FF19}' | '\u{FF21}'..='\u{FF3A}' | '\u{FF41}'..='\u{FF5A}'
        | '\u{1D400}'..='\u{1D7FF}' | '\u{2460}'..='\u{24FF}' | '\u{1F130}'..='\u{1F189}')
        && {
            let mut folded = c.nfkc();
            matches!((folded.next(), folded.next()), (Some(f), None) if f.is_ascii_alphanumeric())
        }
}

/// Characters that take no space: zero-width, joiners, bidi controls, BOM.
fn is_invisible(c: char) -> bool {
    matches!(c,
        '\u{00AD}' | '\u{034F}' | '\u{061C}' | '\u{115F}' | '\u{1160}' | '\u{180E}'
        | '\u{200B}'..='\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{206F}' | '\u{3164}' | '\u{FEFF}' | '\u{FFA0}')
}

/// Chinese, Japanese and Korean script characters.
fn is_cjk(c: char) -> bool {
    matches!(c,
        '\u{3040}'..='\u{30FF}' | '\u{3400}'..='\u{4DBF}' | '\u{4E00}'..='\u{9FFF}'
        | '\u{AC00}'..='\u{D7AF}' | '\u{FF66}'..='\u{FF9F}')
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn from(name: Option<&str>, email: &str) -> Address {
        Address {
            name: name.map(str::to_owned),
            email: email.to_owned(),
        }
    }

    #[test]
    fn a_lookalike_top_level_domain_and_a_full_width_name_are_flagged() {
        // The phishing pattern: "ńovabank.ċom" and a full-width brand name.
        let sender = from(
            Some("Kundenservice ＮＯＶＡＢＡＮＫ"),
            "noreply@xn--ovabank-2jb.xn--om-6la",
        );
        assert_eq!(
            sender_warnings(&[sender]),
            [MessageWarning::LookalikeSender {
                address: "noreply@xn--ovabank-2jb.xn--om-6la".into(),
                unicode_domain: Some("\u{144}ovabank.\u{10b}om".into()),
                resembles: Some("novabank.com".into()),
                name: Some("Kundenservice NOVABANK".into()),
            }]
        );
    }

    #[test]
    fn mixed_script_domains_and_invisible_names_are_flagged() {
        // Cyrillic "а" in "paypal", written in Unicode by an SMTPUTF8 sender.
        let w = sender_warnings(&[from(None, "service@p\u{430}ypal.com")]);
        assert!(matches!(
            &w[..],
            [MessageWarning::LookalikeSender { resembles: Some(r), name: None, .. }] if r == "paypal.com"
        ));
        let w = sender_warnings(&[from(Some("Pay\u{200B}Pal"), "news@shop.example")]);
        assert!(matches!(
            &w[..],
            [MessageWarning::LookalikeSender { unicode_domain: None, name: Some(n), .. }] if n == "PayPal"
        ));
    }

    #[test]
    fn ordinary_senders_pass() {
        for (name, email) in [
            (Some("Jürgen Müller"), "j@m\u{fc}nchen.de"),
            (Some("Jürgen Müller"), "j@xn--mnchen-3ya.de"),
            (Some("Ｍａｉｌ 株式会社"), "info@shop.example.jp"),
            (Some("Ада Лавлейс"), "ada@пример.рф"),
            (Some("Café №5"), "hello@cafe.example"),
            (None, "plain@example.org"),
        ] {
            assert_eq!(
                sender_warnings(&[from(name, email)]),
                [],
                "{name:?} {email}"
            );
        }
    }
}
