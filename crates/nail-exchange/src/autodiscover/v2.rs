//! Autodiscover v2 (JSON):
//! `GET /autodiscover/autodiscover.json/v1.0/<address>?Protocol=EWS` answers `{"Protocol": "EWS", "Url": "…"}`
//! without signing in. Exchange Online answers it (on Microsoft's global
//! endpoint and on hybrid on-premises servers that redirect there), and
//! Exchange Server 2016 CU and later answer it too.

use serde::Deserialize;
use url::Url;

/// Microsoft's global Autodiscover v2 host (Exchange Online).
pub const ONLINE_V2_HOST: &str = "autodiscover-s.outlook.com";

/// The v2 URL for `address` on `host` asking for `protocol` (`EWS`,
/// `AutodiscoverV1`, `ActiveSync`); `None` when the parts do not make a
/// URL.
pub fn url(host: &str, address: &str, protocol: &str) -> Option<Url> {
    let mut url = Url::parse(&format!("https://{host}/")).ok()?;
    url.path_segments_mut()
        .ok()?
        .extend(["autodiscover", "autodiscover.json", "v1.0", address]);
    url.query_pairs_mut().append_pair("Protocol", protocol);
    Some(url)
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "PascalCase")]
struct Answer {
    #[serde(default)]
    protocol: Option<String>,
    #[serde(default)]
    url: Option<String>,
    #[serde(default)]
    error_code: Option<String>,
}

/// The URL a v2 answer names (`None` for errors, other protocols or
/// non-`https` URLs other than loopback ones).
pub fn parse_answer(body: &[u8], protocol: &str) -> Result<String, String> {
    let answer: Answer =
        serde_json::from_slice(body).map_err(|e| format!("not an Autodiscover v2 answer: {e}"))?;
    if let Some(code) = answer.error_code {
        return Err(format!("Autodiscover v2 error {code}"));
    }
    if answer
        .protocol
        .as_deref()
        .is_some_and(|p| !p.eq_ignore_ascii_case(protocol))
    {
        return Err("answer for another protocol".to_owned());
    }
    let url = answer.url.ok_or("answer without URL")?;
    let parsed = Url::parse(&url).map_err(|e| format!("invalid URL: {e}"))?;
    let loopback = parsed
        .host_str()
        .is_some_and(nail_api::exchange::is_loopback_host);
    if parsed.scheme() != "https" && !(parsed.scheme() == "http" && loopback) {
        return Err("refusing a non-https URL".to_owned());
    }
    Ok(url)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_urls() {
        assert_eq!(
            url(ONLINE_V2_HOST, "ada@contoso.example", "EWS")
                .unwrap()
                .as_str(),
            "https://autodiscover-s.outlook.com/autodiscover/autodiscover.json/v1.0/ada@contoso.example?Protocol=EWS"
        );
        // Path characters in the local part are escaped.
        let odd = url("autodiscover.corp.example", "a/b?c@corp.example", "EWS").unwrap();
        assert_eq!(
            odd.path(),
            "/autodiscover/autodiscover.json/v1.0/a%2Fb%3Fc@corp.example"
        );
        assert!(url("bad host", "a@b", "EWS").is_none());
    }

    #[test]
    fn reads_answers() {
        assert_eq!(
            parse_answer(
                br#"{"Protocol":"EWS","Url":"https://outlook.office365.com/EWS/Exchange.asmx"}"#,
                "EWS"
            )
            .unwrap(),
            "https://outlook.office365.com/EWS/Exchange.asmx"
        );
        assert!(
            parse_answer(br#"{"ErrorCode":"InvalidUser","ErrorMessage":"x"}"#, "EWS")
                .unwrap_err()
                .contains("InvalidUser")
        );
        assert!(parse_answer(br#"{"Protocol":"ActiveSync","Url":"https://x/"}"#, "EWS").is_err());
        assert!(
            parse_answer(
                br#"{"Protocol":"EWS","Url":"http://mail.corp.example/EWS"}"#,
                "EWS"
            )
            .is_err()
        );
        assert!(
            parse_answer(
                br#"{"Protocol":"EWS","Url":"http://127.0.0.1:8080/EWS"}"#,
                "EWS"
            )
            .is_ok()
        );
        assert!(parse_answer(b"<html/>", "EWS").is_err());
        assert!(parse_answer(br#"{"Protocol":"EWS"}"#, "EWS").is_err());
    }
}
