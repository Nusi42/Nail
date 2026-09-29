//! `Authentication-Results` (RFC 8601) → [`nail_api::security::AuthResults`]
//! (M2a WP33, ADR 0032).
//!
//! Anyone can write an `Authentication-Results` header, so only headers
//! from a **trusted** `authserv-id` (the account's own receiving server,
//! configured by the user; [`learn_authserv_id`] only suggests one) produce
//! a result, and
//! of those only the ones in the receiving server's own block:
//!
//! * the topmost header with a trusted id, plus further trusted headers
//!   directly below it, up to the next `Received` header. A header below
//!   that `Received` was added by an earlier hop — or forged by the sender
//!   hoping the receiving server does not strip it (RFC 8601 section 5
//!   says it should).
//! * DKIM results are collected (one per signature); SPF, DMARC and ARC
//!   take the first result found. When the trusted block has no `arc`
//!   result, the newest `ARC-Authentication-Results` of a trusted id is
//!   used for it.
//! * `aligned` is computed against the `From` domain with relaxed
//!   alignment (organizational domains, see `domain`); it is
//!   informational, the server's `dmarc=` verdict is what counts.
//!
//! The parser handles comments (nested, escaped), quoted strings, spaces
//! around `=` / `.` / `/`, method versions (`dkim/1`), `reason=` and
//! property values that are addresses (`smtp.mailfrom=user@example.org`).
//! It never panics; malformed parts are skipped.

use nail_api::security::{AuthMethodResult, AuthResults, AuthVerdict};

use crate::domain;

/// One `resinfo`: `method=result reason=… ptype.property=value …`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultInfo {
    /// Method, lowercase (`dkim`, `spf`, `dmarc`, `arc`, `auth`, …).
    pub method: String,
    /// Method version, if given (`dkim/1`).
    pub method_version: Option<String>,
    /// Result, lowercase (`pass`, `fail`, …).
    pub result: String,
    /// The `reason=` value.
    pub reason: Option<String>,
    /// Properties as (`ptype`, `property`, value); ptype and property
    /// lowercase.
    pub properties: Vec<(String, String, String)>,
}

impl ResultInfo {
    /// The value of `ptype.property`.
    pub fn property(&self, ptype: &str, property: &str) -> Option<&str> {
        self.properties
            .iter()
            .find(|(t, p, _)| t == ptype && p == property)
            .map(|(_, _, v)| v.as_str())
    }
}

/// A parsed `Authentication-Results` (or `ARC-Authentication-Results`)
/// header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedHeader {
    /// The `authserv-id`, lowercase.
    pub authserv_id: String,
    /// ARC instance (`i=`), only for `ARC-Authentication-Results`.
    pub instance: Option<u32>,
    /// The results (empty for `none`).
    pub results: Vec<ResultInfo>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Token {
    Word(String),
    Quoted(String),
    Eq,
    Semi,
}

/// Splits a header value into words, quoted strings, `=` and `;`,
/// dropping comments and whitespace.
fn tokenize(value: &str) -> Vec<Token> {
    let mut out = Vec::new();
    let mut chars = value.chars().peekable();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut Vec<Token>| {
        if !word.is_empty() {
            out.push(Token::Word(std::mem::take(word)));
        }
    };
    while let Some(c) = chars.next() {
        match c {
            '(' => {
                flush(&mut word, &mut out);
                let mut depth = 1;
                while depth > 0 {
                    match chars.next() {
                        Some('\\') => {
                            chars.next();
                        }
                        Some('(') => depth += 1,
                        Some(')') => depth -= 1,
                        Some(_) => {}
                        None => break,
                    }
                }
            }
            '"' => {
                flush(&mut word, &mut out);
                let mut quoted = String::new();
                loop {
                    match chars.next() {
                        Some('\\') => {
                            if let Some(c) = chars.next() {
                                quoted.push(c);
                            }
                        }
                        Some('"') | None => break,
                        Some(c) => quoted.push(c),
                    }
                }
                out.push(Token::Quoted(quoted));
            }
            '=' => {
                flush(&mut word, &mut out);
                out.push(Token::Eq);
            }
            ';' => {
                flush(&mut word, &mut out);
                out.push(Token::Semi);
            }
            c if c.is_whitespace() => flush(&mut word, &mut out),
            c => word.push(c),
        }
    }
    flush(&mut word, &mut out);
    // `ptype . property` and `method / version` may have spaces around the
    // separator: glue words that end or start with `.` / `/` — only in key
    // position (not right after `=`, where `example.org.` is a value).
    let mut glued: Vec<Token> = Vec::with_capacity(out.len());
    for token in out {
        let n = glued.len();
        let key_position = n < 2 || glued[n - 2] != Token::Eq;
        match (glued.last_mut(), &token) {
            (Some(Token::Word(prev)), Token::Word(next))
                if key_position && (prev.ends_with(['.', '/']) || next.starts_with(['.', '/'])) =>
            {
                prev.push_str(next);
            }
            _ => glued.push(token),
        }
    }
    glued
}

fn text(token: &Token) -> Option<&str> {
    match token {
        Token::Word(w) | Token::Quoted(w) => Some(w),
        Token::Eq | Token::Semi => None,
    }
}

/// `key=value` pairs and lone words of one `;`-separated segment.
fn pairs(segment: &[Token]) -> Vec<(String, Option<String>)> {
    let mut out = Vec::new();
    let mut i = 0;
    while i < segment.len() {
        let Some(key) = text(&segment[i]) else {
            i += 1;
            continue;
        };
        if segment.get(i + 1) == Some(&Token::Eq) {
            let value = segment.get(i + 2).and_then(text).map(str::to_owned);
            out.push((key.to_owned(), value));
            i += 3;
        } else {
            out.push((key.to_owned(), None));
            i += 1;
        }
    }
    out
}

fn result_info(segment: &[Token]) -> Option<ResultInfo> {
    let mut pairs = pairs(segment).into_iter();
    let (method, result) = pairs.next()?;
    let result = result?.to_ascii_lowercase();
    let (method, method_version) = match method.split_once('/') {
        Some((m, v)) => (m.to_owned(), Some(v.to_owned())),
        None => (method, None),
    };
    let mut info = ResultInfo {
        method: method.to_ascii_lowercase(),
        method_version,
        result,
        reason: None,
        properties: Vec::new(),
    };
    for (key, value) in pairs {
        let Some(value) = value else { continue };
        let key = key.to_ascii_lowercase();
        if key == "reason" {
            info.reason = Some(value);
        } else if let Some((ptype, property)) = key.split_once('.') {
            info.properties
                .push((ptype.to_owned(), property.to_owned(), value));
        }
    }
    Some(info)
}

/// Parses an `Authentication-Results` header value. `None` when it has no
/// `authserv-id`.
pub fn parse_header(value: &str) -> Option<ParsedHeader> {
    parse(value, false)
}

/// Parses an `ARC-Authentication-Results` header value (`i=N;` first).
pub fn parse_arc_header(value: &str) -> Option<ParsedHeader> {
    parse(value, true)
}

fn parse(value: &str, arc: bool) -> Option<ParsedHeader> {
    let tokens = tokenize(value);
    let mut segments = tokens.split(|t| *t == Token::Semi);
    let mut instance = None;
    if arc {
        let first = pairs(segments.next()?);
        let (key, value) = first.first()?;
        if !key.eq_ignore_ascii_case("i") {
            return None;
        }
        instance = value.as_deref().and_then(|v| v.parse().ok());
    }
    let head = segments.next()?;
    let authserv_id = head.first().and_then(text)?.to_ascii_lowercase();
    // A following all-digit word is the version; only version 1 exists.
    if let Some(version) = head.get(1).and_then(text)
        && version.chars().all(|c| c.is_ascii_digit())
        && version != "1"
    {
        return None;
    }
    let results = segments
        .filter(|s| !s.is_empty())
        .filter_map(result_info)
        .filter(|r| r.method != "none")
        .collect();
    Some(ParsedHeader {
        authserv_id,
        instance,
        results,
    })
}

fn verdict(result: &str) -> AuthVerdict {
    match result {
        "pass" => AuthVerdict::Pass,
        "fail" | "hardfail" => AuthVerdict::Fail,
        "softfail" => AuthVerdict::Softfail,
        "neutral" => AuthVerdict::Neutral,
        "none" => AuthVerdict::None,
        "temperror" => AuthVerdict::Temperror,
        "permerror" => AuthVerdict::Permerror,
        "policy" => AuthVerdict::Policy,
        _ => AuthVerdict::Unknown,
    }
}

fn method_result(info: &ResultInfo, from_domain: Option<&str>) -> AuthMethodResult {
    let domain = match info.method.as_str() {
        "dkim" => info
            .property("header", "d")
            .or_else(|| info.property("header", "i"))
            .and_then(domain::domain_of),
        "spf" => info
            .property("smtp", "mailfrom")
            .or_else(|| info.property("smtp", "helo"))
            .and_then(domain::domain_of),
        "dmarc" => info.property("header", "from").and_then(domain::domain_of),
        _ => None,
    };
    let aligned = match (&domain, from_domain) {
        (Some(d), Some(from)) => Some(domain::aligned(d, from)),
        _ => None,
    };
    AuthMethodResult {
        verdict: verdict(&info.result),
        domain,
        selector: info.property("header", "s").map(str::to_owned),
        aligned,
        reason: info.reason.clone(),
    }
}

fn is_trusted(id: &str, trusted: &[String]) -> bool {
    trusted.iter().any(|t| t.eq_ignore_ascii_case(id.trim()))
}

/// The trusted sender-authentication results of a message, from its
/// top-level header fields in order (as `crate::header_fields` returns
/// them). `from` is the `From` address (for alignment). `None` when no
/// trusted header exists.
pub fn auth_results(
    headers: &[(String, String)],
    from: Option<&str>,
    trusted: &[String],
) -> Option<AuthResults> {
    let from_domain = from.and_then(domain::domain_of);
    let mut block: Option<(String, Vec<ResultInfo>)> = None;
    let mut arc_fallback: Option<(u32, ResultInfo)> = None;
    let mut block_closed = false;
    for (name, value) in headers {
        if name.eq_ignore_ascii_case("Received") {
            if block.is_some() {
                block_closed = true;
            }
            continue;
        }
        if name.eq_ignore_ascii_case("ARC-Authentication-Results") {
            if let Some(parsed) = parse_arc_header(value)
                && is_trusted(&parsed.authserv_id, trusted)
                && let Some(arc) = parsed.results.into_iter().find(|r| r.method == "arc")
            {
                let instance = parsed.instance.unwrap_or(0);
                if arc_fallback.as_ref().is_none_or(|(i, _)| instance > *i) {
                    arc_fallback = Some((instance, arc));
                }
            }
            continue;
        }
        if !name.eq_ignore_ascii_case("Authentication-Results") || block_closed {
            continue;
        }
        let Some(parsed) = parse_header(value) else {
            continue;
        };
        if !is_trusted(&parsed.authserv_id, trusted) {
            continue;
        }
        match &mut block {
            None => block = Some((parsed.authserv_id, parsed.results)),
            Some((_, results)) => results.extend(parsed.results),
        }
    }
    let (authserv_id, results) = block?;
    let mut out = AuthResults {
        authserv_id,
        dkim: Vec::new(),
        spf: None,
        dmarc: None,
        arc: None,
    };
    for info in &results {
        let result = method_result(info, from_domain.as_deref());
        match info.method.as_str() {
            "dkim" => out.dkim.push(result),
            "spf" if out.spf.is_none() => out.spf = Some(result),
            "dmarc" if out.dmarc.is_none() => out.dmarc = Some(result),
            "arc" if out.arc.is_none() => out.arc = Some(result),
            _ => {}
        }
    }
    if out.arc.is_none() {
        out.arc = arc_fallback.map(|(_, info)| method_result(&info, from_domain.as_deref()));
    }
    Some(out)
}

/// Parses a raw message and returns its trusted results (see
/// [`auth_results`]).
pub fn auth_results_of_raw(raw: &[u8], trusted: &[String]) -> Option<AuthResults> {
    let headers = crate::header_fields(raw);
    let from = headers
        .iter()
        .find(|(n, _)| n.eq_ignore_ascii_case("From"))
        .map(|(_, v)| v.clone());
    let from = from.as_deref().and_then(address_in);
    auth_results(&headers, from.as_deref(), trusted)
}

/// The address in a `From` value (`Name <a@b>` or `a@b`).
fn address_in(value: &str) -> Option<String> {
    if let (Some(start), Some(end)) = (value.rfind('<'), value.rfind('>'))
        && start < end
    {
        return Some(value[start + 1..end].trim().to_owned());
    }
    value
        .split(|c: char| c.is_whitespace() || c == ',')
        .find(|w| w.contains('@'))
        .map(|w| {
            w.trim_matches(|c| c == '"' || c == '<' || c == '>')
                .to_owned()
        })
}

/// The host after `by` in a `Received` value, when it is a DNS name (not
/// an address literal).
fn received_by(value: &str) -> Option<String> {
    let mut words = value.split_whitespace();
    while let Some(word) = words.next() {
        if !word.eq_ignore_ascii_case("by") {
            continue;
        }
        let host = words.next()?.trim_end_matches([';', ',']);
        let is_name = host.contains('.')
            && host.chars().any(|c| c.is_ascii_alphabetic())
            && !host.contains([':', '[', ']']);
        return if is_name {
            domain::normalize_host(host)
        } else {
            None
        };
    }
    None
}

/// The receiving server of a message: the `by` host of the topmost
/// (newest) `Received` header that names one by DNS name. That header was
/// added by the account's own mail system, never by the sender.
fn receiving_host(headers: &[(String, String)]) -> Option<String> {
    headers
        .iter()
        .filter(|(n, _)| n.eq_ignore_ascii_case("Received"))
        .find_map(|(_, v)| received_by(v))
}

/// A **suggestion** for the `authserv-id` of an account's receiving
/// server, from recent mail of the account (header lists of messages it
/// received, newest first): the id of the topmost `Authentication-Results`
/// header, when the same id is on top in at least `min_share` (0..=1) of
/// at least three messages. A message only counts for an id that belongs
/// to its receiving server (same organizational domain as the `by` host of
/// its newest `Received` header), so ids made up by senders
/// (`mx.attacker.example`) are not proposed.
///
/// This is never trusted by itself: when the receiving server adds no
/// `Authentication-Results`, a sender can still name the server's own
/// domain. The user confirms it (it becomes the configured `authserv-id`).
pub fn learn_authserv_id<'a>(
    messages: impl IntoIterator<Item = &'a [(String, String)]>,
    min_share: f64,
) -> Option<String> {
    let mut counts: Vec<(String, usize)> = Vec::new();
    let mut total = 0usize;
    for headers in messages {
        total += 1;
        let top = headers
            .iter()
            .find(|(n, _)| n.eq_ignore_ascii_case("Authentication-Results"))
            .and_then(|(_, v)| parse_header(v))
            .map(|p| p.authserv_id);
        let receiver = receiving_host(headers);
        let Some(id) = top.filter(|id| receiver.as_deref().is_some_and(|r| domain::aligned(id, r)))
        else {
            continue;
        };
        match counts.iter_mut().find(|(i, _)| i.eq_ignore_ascii_case(&id)) {
            Some((_, n)) => *n += 1,
            None => counts.push((id, 1)),
        }
    }
    if total < 3 {
        return None;
    }
    let (id, n) = counts.into_iter().max_by_key(|(_, n)| *n)?;
    #[allow(clippy::cast_precision_loss)]
    let share = n as f64 / total as f64;
    (share >= min_share).then_some(id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn h(name: &str, value: &str) -> (String, String) {
        (name.to_owned(), value.to_owned())
    }

    fn trusted() -> Vec<String> {
        vec!["mx.example.net".to_owned()]
    }

    #[test]
    fn parses_the_rfc_8601_examples() {
        // RFC 8601 appendix B.3.
        let p = parse_header("example.com; spf=pass smtp.mailfrom=example.net").unwrap();
        assert_eq!(p.authserv_id, "example.com");
        assert_eq!(p.results.len(), 1);
        assert_eq!(p.results[0].method, "spf");
        assert_eq!(p.results[0].result, "pass");
        assert_eq!(
            p.results[0].property("smtp", "mailfrom"),
            Some("example.net")
        );

        // B.4 with comments and a version.
        let p = parse_header(
            "example.com 1 (Example!); auth=pass (cram-md5) smtp.auth=sender@example.net; \
             spf=pass smtp.mailfrom=example.net",
        )
        .unwrap();
        assert_eq!(p.authserv_id, "example.com");
        assert_eq!(p.results.len(), 2);
        assert_eq!(
            p.results[0].property("smtp", "auth"),
            Some("sender@example.net")
        );

        // B.5: several methods, a method version, reason and quoted value.
        let p = parse_header(
            "example.com;\r\n  dkim/1=pass (good signature) header.d=mail-router.example.net;\r\n \
             dkim=fail reason=\"bad signature\" header.i=@newyork.example.com",
        )
        .unwrap();
        assert_eq!(p.results[0].method_version.as_deref(), Some("1"));
        assert_eq!(p.results[1].reason.as_deref(), Some("bad signature"));
        assert_eq!(
            p.results[1].property("header", "i"),
            Some("@newyork.example.com")
        );

        // "none".
        let p = parse_header("example.org 1; none").unwrap();
        assert!(p.results.is_empty());
        // An unknown version is not understood.
        assert!(parse_header("example.org 2; spf=pass").is_none());
        assert!(parse_header("").is_none());
    }

    #[test]
    fn tolerates_odd_spacing_nested_comments_and_quoting() {
        let p = parse_header(
            "\"mx.example.net\" ; dkim = pass (nested (comment) \\) here) header . d = \
             example.org header.s=sel1 ; spf=SoftFail smtp.mailfrom = \"a b\"@example.org",
        )
        .unwrap();
        assert_eq!(p.authserv_id, "mx.example.net");
        assert_eq!(p.results[0].method, "dkim");
        assert_eq!(p.results[0].property("header", "d"), Some("example.org"));
        assert_eq!(p.results[0].property("header", "s"), Some("sel1"));
        assert_eq!(p.results[1].result, "softfail");
        // A value ending in a dot is not glued to the next key.
        let p = parse_header("x; spf=pass smtp.mailfrom=example.org. smtp.helo=relay").unwrap();
        assert_eq!(
            p.results[0].property("smtp", "mailfrom"),
            Some("example.org.")
        );
        assert_eq!(p.results[0].property("smtp", "helo"), Some("relay"));
        // Garbage never panics.
        for junk in ["(((", "\"", ";;;;", "a;=;=", "x; =pass", "x; dkim=", "=;"] {
            let _ = parse_header(junk);
            let _ = parse_arc_header(junk);
        }
    }

    #[test]
    fn maps_a_trusted_header_with_alignment() {
        let headers = vec![
            h("Received", "from mx by store"),
            h(
                "Authentication-Results",
                "mx.example.net; dkim=pass header.d=mail.shop.example header.s=s1; \
                 dkim=fail header.d=other.example; spf=pass smtp.mailfrom=bounce@shop.example; \
                 dmarc=pass header.from=shop.example; arc=none",
            ),
            h("Received", "from sender by mx"),
        ];
        let auth = auth_results(&headers, Some("news@shop.example"), &trusted()).unwrap();
        assert_eq!(auth.authserv_id, "mx.example.net");
        assert_eq!(auth.dkim.len(), 2);
        assert_eq!(auth.dkim[0].verdict, AuthVerdict::Pass);
        assert_eq!(auth.dkim[0].domain.as_deref(), Some("mail.shop.example"));
        assert_eq!(auth.dkim[0].selector.as_deref(), Some("s1"));
        assert_eq!(auth.dkim[0].aligned, Some(true));
        assert_eq!(auth.dkim[1].aligned, Some(false));
        let spf = auth.spf.as_ref().unwrap();
        assert_eq!(spf.domain.as_deref(), Some("shop.example"));
        assert_eq!(spf.aligned, Some(true));
        assert_eq!(auth.dmarc.as_ref().unwrap().verdict, AuthVerdict::Pass);
        assert_eq!(auth.arc.as_ref().unwrap().verdict, AuthVerdict::None);
        assert_eq!(auth.summary(), AuthVerdict::Pass);
    }

    #[test]
    fn untrusted_and_forged_headers_are_ignored() {
        // Only an untrusted id: nothing.
        let headers = vec![h(
            "Authentication-Results",
            "evil.example; dkim=pass header.d=bank.example; dmarc=pass header.from=bank.example",
        )];
        assert_eq!(
            auth_results(&headers, Some("x@bank.example"), &trusted()),
            None
        );

        // The real server's verdict on top; a forged "trusted" header the
        // sender placed below the server's Received line is ignored.
        let headers = vec![
            h(
                "Authentication-Results",
                "mx.example.net; dkim=none; spf=fail smtp.mailfrom=bank.example; \
                 dmarc=fail header.from=bank.example",
            ),
            h("Received", "from attacker by mx.example.net"),
            h(
                "Authentication-Results",
                "mx.example.net; dkim=pass header.d=bank.example; dmarc=pass header.from=bank.example",
            ),
        ];
        let auth = auth_results(&headers, Some("x@bank.example"), &trusted()).unwrap();
        assert_eq!(auth.dmarc.unwrap().verdict, AuthVerdict::Fail);
        assert_eq!(auth.dkim.len(), 1);
        assert_eq!(auth.dkim[0].verdict, AuthVerdict::None);
        assert_eq!(auth.spf.unwrap().verdict, AuthVerdict::Fail);
    }

    #[test]
    fn adjacent_trusted_headers_are_merged_and_case_is_ignored() {
        let headers = vec![
            h(
                "Authentication-Results",
                "MX.Example.NET; spf=softfail smtp.helo=relay.example",
            ),
            h(
                "Authentication-Results",
                "mx.example.net; dkim=temperror header.d=relay.example",
            ),
            h("Authentication-Results", "other.example; dmarc=pass"),
        ];
        let auth = auth_results(&headers, None, &trusted()).unwrap();
        assert_eq!(auth.spf.as_ref().unwrap().verdict, AuthVerdict::Softfail);
        assert_eq!(
            auth.spf.as_ref().unwrap().domain.as_deref(),
            Some("relay.example")
        );
        assert_eq!(auth.spf.as_ref().unwrap().aligned, None);
        assert_eq!(auth.dkim[0].verdict, AuthVerdict::Temperror);
        assert!(auth.dmarc.is_none());
        assert_eq!(auth.summary(), AuthVerdict::Temperror);
    }

    #[test]
    fn arc_results_fall_back_to_the_newest_trusted_arc_header() {
        let headers = vec![
            h(
                "ARC-Authentication-Results",
                "i=2; mx.example.net; arc=pass (i=1)",
            ),
            h(
                "ARC-Authentication-Results",
                "i=1; mx.example.net; arc=none",
            ),
            h(
                "ARC-Authentication-Results",
                "i=3; untrusted.example; arc=fail",
            ),
            h(
                "Authentication-Results",
                "mx.example.net; dkim=fail header.d=list.example",
            ),
        ];
        let auth = auth_results(&headers, None, &trusted()).unwrap();
        assert_eq!(auth.arc.unwrap().verdict, AuthVerdict::Pass);
        assert_eq!(
            parse_arc_header("i=2; mx.example.net; arc=pass")
                .unwrap()
                .instance,
            Some(2)
        );
        assert!(parse_arc_header("mx.example.net; arc=pass").is_none());
    }

    #[test]
    fn verdict_values() {
        for (text, v) in [
            ("pass", AuthVerdict::Pass),
            ("hardfail", AuthVerdict::Fail),
            ("permerror", AuthVerdict::Permerror),
            ("policy", AuthVerdict::Policy),
            ("neutral", AuthVerdict::Neutral),
            ("bogus", AuthVerdict::Unknown),
        ] {
            assert_eq!(verdict(text), v);
        }
    }

    #[test]
    fn from_raw_message() {
        let raw =
            b"Authentication-Results: mx.example.net;\r\n dkim=pass header.d=example.org;\r\n\
 dmarc=pass header.from=example.org\r\nReceived: from x by mx.example.net\r\n\
From: \"Ada\" <ada@mail.example.org>\r\nSubject: hi\r\n\r\nbody";
        let auth = auth_results_of_raw(raw, &trusted()).unwrap();
        assert_eq!(auth.dkim[0].aligned, Some(true));
        assert_eq!(auth.summary(), AuthVerdict::Pass);
        assert_eq!(auth_results_of_raw(raw, &["other".into()]), None);
        assert_eq!(
            address_in("ada@example.org").as_deref(),
            Some("ada@example.org")
        );
        assert_eq!(address_in("Ada Lovelace").as_deref(), None);
    }

    #[test]
    fn learns_the_receiving_servers_id() {
        let top = |id: &str| {
            vec![
                h(
                    "Received",
                    "from mail.sender.example (mail.sender.example [192.0.2.1]) by mx1.example.net with ESMTPS; Tue, 29 Sep 2026 08:00:00 +0000",
                ),
                h("Authentication-Results", &format!("{id}; spf=pass")),
                h("Authentication-Results", "upstream.example; dkim=pass"),
            ]
        };
        let a = top("mx.example.net");
        let b = top("mx.example.net");
        let c = top("mx.example.net");
        let d = top("forged.example");
        let none = vec![h("Subject", "no results")];
        let msgs: Vec<&[(String, String)]> = vec![&a, &b, &c, &d, &none];
        assert_eq!(
            learn_authserv_id(msgs.clone(), 0.5).as_deref(),
            Some("mx.example.net")
        );
        assert_eq!(learn_authserv_id(msgs, 0.8), None);
        assert_eq!(
            learn_authserv_id(vec![&a[..], &b[..]], 0.5),
            None,
            "too few"
        );
    }

    #[test]
    fn ids_made_up_by_senders_are_not_suggested() {
        // The receiving server adds no results; every message carries the
        // sender's own forged header below the server's Received line.
        let forged = vec![
            h(
                "Received",
                "from evil.example by mail.small.example with ESMTP; Tue, 29 Sep 2026",
            ),
            h(
                "Authentication-Results",
                "mx.attacker.example; dkim=pass; dmarc=pass header.from=bank.example",
            ),
        ];
        let msgs: Vec<&[(String, String)]> = vec![&forged; 20];
        assert_eq!(learn_authserv_id(msgs, 0.8), None);
        // Without any Received header there is no receiving server.
        let bare = [h("Authentication-Results", "mx.example.net; spf=pass")];
        assert_eq!(learn_authserv_id(vec![&bare[..]; 5], 0.8), None);
        // An address literal after "by" does not name the server; the next
        // Received with a DNS name does (Gmail-style internal hop on top).
        let gmail = [
            h(
                "Received",
                "by 2002:a05:7300:1234 with SMTP id x; Tue, 29 Sep 2026",
            ),
            h(
                "Received",
                "from mail.sender.example by mx.google.com with ESMTPS id y",
            ),
            h(
                "Authentication-Results",
                "mx.google.com; dkim=pass header.d=sender.example",
            ),
        ];
        assert_eq!(
            learn_authserv_id(vec![&gmail[..]; 3], 0.8).as_deref(),
            Some("mx.google.com")
        );
        assert_eq!(received_by("from x by [192.0.2.1]"), None);
        assert_eq!(
            received_by("from x (y) BY Mx.Example.NET;"),
            Some("mx.example.net".into())
        );
    }
}
