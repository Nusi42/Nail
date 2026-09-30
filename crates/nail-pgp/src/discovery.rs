//! OpenPGP key discovery: Web Key Directory (draft-koch-openpgp-webkey-
//! service, advanced then direct method) and keys.openpgp.org (VKS API).
//!
//! HTTP goes through [`HttpClient`], which the core implements with its
//! HTTP stack (TLS, proxy, timeouts, size limit); tests use a fake.
//! Nothing is imported here: [`lookup`] returns candidates
//! ([`KeyLookupCandidate`]) and [`import_candidate`] stores one when the
//! user (or the automatic lookup policy) accepts it.

use nail_api::ErrorCode;
use nail_api::engine::{BoxFuture, EngineError};
use nail_api::security::{
    CryptoKey, CryptoTechnology, KeyLookupCandidate, KeyLookupFailure, KeyLookupSource, KeySource,
};
use sha1::Digest;

use crate::error::{Problem, Result};
use crate::key::{self, KeyInfo};
use crate::keyring::{Keyring, rfc3339, to_crypto_key};

/// Largest response accepted (a key with a few subkeys is a few KiB).
pub const MAX_RESPONSE_BYTES: usize = 1 << 20;

/// keys.openpgp.org's VKS base URL.
pub const KEYS_OPENPGP_ORG: &str = "https://keys.openpgp.org";

/// An HTTP response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpResponse {
    /// Status code.
    pub status: u16,
    /// Body (the implementation stops reading after
    /// [`MAX_RESPONSE_BYTES`]).
    pub body: Vec<u8>,
}

/// HTTPS GET, supplied by the core (tests pass a fake). Implementations
/// must use TLS with certificate validation, follow at most a few
/// redirects (HTTPS only) and time out.
pub trait HttpClient: Send + Sync {
    /// GETs `url`. Network / TLS problems are errors; HTTP statuses are
    /// responses.
    fn get<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Result<HttpResponse, EngineError>>;
}

/// z-base-32 (RFC 6189 §5.1.6) as WKD uses it.
pub fn zbase32(data: &[u8]) -> String {
    const ALPHABET: &[u8; 32] = b"ybndrfg8ejkmcpqxot1uwisza345h769";
    let mut out = String::with_capacity(data.len() * 8 / 5 + 1);
    let mut buffer: u32 = 0;
    let mut bits = 0;
    for &byte in data {
        buffer = (buffer << 8) | u32::from(byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((buffer >> bits) & 31) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((buffer << (5 - bits)) & 31) as usize] as char);
    }
    out
}

/// Percent-encodes everything but RFC 3986 unreserved characters.
pub fn percent_encode(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for byte in text.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            out.push(byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// The WKD URLs of an address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WkdUrls {
    /// `https://openpgpkey.<domain>/.well-known/openpgpkey/<domain>/hu/<hash>?l=<local>`.
    pub advanced: String,
    /// `https://<domain>/.well-known/openpgpkey/hu/<hash>?l=<local>`.
    pub direct: String,
}

fn split_address(email: &str) -> Result<(&str, String)> {
    let email = email.trim();
    let (local, domain) = email
        .rsplit_once('@')
        .filter(|(l, d)| !l.is_empty() && !d.is_empty())
        .ok_or_else(|| Problem::NotAnAddress {
            address: email.to_owned(),
        })?;
    let domain = domain.to_ascii_lowercase();
    if !domain
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
    {
        return Err(Problem::UnsupportedDomain { domain }.into());
    }
    Ok((local, domain))
}

/// The WKD URLs for `email` (the hash is over the lowercased local part).
pub fn wkd_urls(email: &str) -> Result<WkdUrls> {
    let (local, domain) = split_address(email)?;
    let hash = zbase32(&sha1::Sha1::digest(local.to_ascii_lowercase().as_bytes()));
    let l = percent_encode(local);
    Ok(WkdUrls {
        advanced: format!(
            "https://openpgpkey.{domain}/.well-known/openpgpkey/{domain}/hu/{hash}?l={l}"
        ),
        direct: format!("https://{domain}/.well-known/openpgpkey/hu/{hash}?l={l}"),
    })
}

/// The VKS by-email URL.
pub fn keys_openpgp_org_url(email: &str) -> Result<String> {
    split_address(email)?;
    Ok(format!(
        "{KEYS_OPENPGP_ORG}/vks/v1/by-email/{}",
        percent_encode(email.trim())
    ))
}

/// Result of [`lookup`].
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct LookupResult {
    /// Keys found, valid ones first, then newest.
    pub candidates: Vec<KeyLookupCandidate>,
    /// Sources that failed (not finding anything is not a failure).
    pub failures: Vec<KeyLookupFailure>,
}

enum Fetch {
    Found(Vec<u8>),
    NotFound,
    Failed(ErrorCode, String),
}

async fn fetch(http: &dyn HttpClient, url: &str) -> Fetch {
    match http.get(url).await {
        Ok(response) if response.status == 200 => {
            if response.body.len() > MAX_RESPONSE_BYTES {
                Fetch::Failed(ErrorCode::TooLarge, format!("{url}: response too large"))
            } else {
                Fetch::Found(response.body)
            }
        }
        Ok(response) if response.status == 404 || response.status == 410 => Fetch::NotFound,
        Ok(response) => Fetch::Failed(
            ErrorCode::Network,
            format!("{url}: HTTP {}", response.status),
        ),
        Err(e) => Fetch::Failed(e.code, format!("{url}: {}", e.message)),
    }
}

/// Keys in `data` that carry a user id for `email`, as candidates.
fn candidates_from(
    data: &[u8],
    email: &str,
    source: KeyLookupSource,
    now: i64,
) -> Vec<(bool, i64, KeyLookupCandidate)> {
    let email = email.trim().to_lowercase();
    let (keys, _) = key::parse_keys(data);
    keys.into_iter()
        .filter_map(|parsed| {
            // Never take secret material from the network.
            let mut public = parsed.public();
            key::strip_foreign(&mut public);
            let info = KeyInfo::analyze(&public).ok()?;
            if !info.emails.contains(&email) {
                return None;
            }
            let armored = key::to_armored(&public).ok()?;
            Some((
                info.valid_at(now) && info.can_encrypt(now),
                info.created_at,
                KeyLookupCandidate {
                    source,
                    fingerprint: info.fingerprint.clone(),
                    user_ids: info.user_ids.clone(),
                    algorithm: info.algorithm.clone(),
                    created_at: rfc3339(info.created_at),
                    expires_at: info.expires_at.map(rfc3339),
                    known: None,
                    armored,
                },
            ))
        })
        .collect()
}

/// Looks `email` up in `sources` (in order). Candidates are deduplicated
/// by fingerprint (first source wins).
pub async fn lookup(
    http: &dyn HttpClient,
    email: &str,
    sources: &[KeyLookupSource],
    now: i64,
) -> Result<LookupResult> {
    let urls = wkd_urls(email)?;
    let vks = keys_openpgp_org_url(email)?;
    let mut found: Vec<(bool, i64, KeyLookupCandidate)> = Vec::new();
    let mut failures = Vec::new();
    for source in sources {
        let outcome = match source {
            KeyLookupSource::Wkd => match fetch(http, &urls.advanced).await {
                Fetch::Found(data) => Fetch::Found(data),
                // No advanced directory (or no host): the direct method.
                Fetch::NotFound | Fetch::Failed(..) => fetch(http, &urls.direct).await,
            },
            KeyLookupSource::KeysOpenpgpOrg => fetch(http, &vks).await,
        };
        match outcome {
            Fetch::Found(data) => {
                for candidate in candidates_from(&data, email, *source, now) {
                    if !found
                        .iter()
                        .any(|(_, _, c)| c.fingerprint == candidate.2.fingerprint)
                    {
                        found.push(candidate);
                    }
                }
            }
            Fetch::NotFound => {}
            Fetch::Failed(code, message) => failures.push(KeyLookupFailure {
                source: *source,
                code,
                message,
                localized: None,
            }),
        }
    }
    found.sort_by_key(|(valid, created, _)| {
        (std::cmp::Reverse(*valid), std::cmp::Reverse(*created))
    });
    Ok(LookupResult {
        candidates: found.into_iter().map(|(_, _, c)| c).collect(),
        failures,
    })
}

/// Fills [`KeyLookupCandidate::known`] from the keyring.
pub fn mark_known(keyring: &Keyring<'_>, candidates: &mut [KeyLookupCandidate]) -> Result<()> {
    for candidate in candidates {
        candidate.known = keyring
            .store()
            .find_key(CryptoTechnology::OpenPgp, &candidate.fingerprint)?
            .map(|k| k.id);
    }
    Ok(())
}

/// Stores a looked-up key (source `wkd` / `keyserver`).
pub fn import_candidate(
    keyring: &Keyring<'_>,
    candidate: &KeyLookupCandidate,
) -> Result<CryptoKey> {
    let (keys, _) = key::parse_keys(candidate.armored.as_bytes());
    let public = keys
        .into_iter()
        .next()
        .map(|k| k.public())
        .ok_or(Problem::CandidateWithoutKey)?;
    let source = match candidate.source {
        KeyLookupSource::Wkd => KeySource::Wkd,
        KeyLookupSource::KeysOpenpgpOrg => KeySource::Keyserver,
    };
    let (stored, _) = keyring.store_public(&public, source)?;
    Ok(to_crypto_key(&stored, keyring.now()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keyring::UnlockCache;
    use crate::testutil::fixture;
    use nail_api::engine::MemorySecretStore;
    use pretty_assertions::assert_eq;
    use std::collections::HashMap;
    use std::sync::Mutex;

    /// Answers from a map; records requested URLs.
    #[derive(Default)]
    struct FakeHttp {
        answers: HashMap<String, Result<HttpResponse, EngineError>>,
        requested: Mutex<Vec<String>>,
    }

    impl FakeHttp {
        fn answer(mut self, url: &str, status: u16, body: &[u8]) -> Self {
            self.answers.insert(
                url.to_owned(),
                Ok(HttpResponse {
                    status,
                    body: body.to_vec(),
                }),
            );
            self
        }

        fn fail(mut self, url: &str) -> Self {
            self.answers.insert(
                url.to_owned(),
                Err(EngineError::temporary(
                    ErrorCode::Network,
                    "connection refused",
                )),
            );
            self
        }
    }

    impl HttpClient for FakeHttp {
        fn get<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Result<HttpResponse, EngineError>> {
            self.requested.lock().unwrap().push(url.to_owned());
            let answer = self.answers.get(url).cloned().unwrap_or(Ok(HttpResponse {
                status: 404,
                body: Vec::new(),
            }));
            Box::pin(async move { answer })
        }
    }

    fn block_on<F: Future>(future: F) -> F::Output {
        let mut future = std::pin::pin!(future);
        let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
        loop {
            if let std::task::Poll::Ready(v) = future.as_mut().poll(&mut cx) {
                return v;
            }
        }
    }

    const NOW: i64 = 1_790_000_000;
    const BOTH: [KeyLookupSource; 2] = [KeyLookupSource::Wkd, KeyLookupSource::KeysOpenpgpOrg];

    /// M2d review: these used to reach the UI as English `detail` text of
    /// "The OpenPGP data is not valid".
    #[test]
    fn bad_addresses_are_problems_with_their_own_text() {
        for (email, id) in [
            ("@example.org", "pgp-problem-not-an-address"),
            ("ada@", "pgp-problem-not-an-address"),
            ("a@münchen.de", "pgp-problem-unsupported-domain"),
        ] {
            let err = wkd_urls(email).unwrap_err();
            assert!(matches!(err, crate::Error::Problem(_)), "{email}: {err:?}");
            let localized = err.localized().unwrap();
            assert_eq!(localized.id, id, "{email}");
            assert!(!localized.args.contains_key("detail"), "{email}");
            assert!(keys_openpgp_org_url(email).is_err());
        }
    }

    #[test]
    fn zbase32_and_wkd_urls_match_the_draft_example() {
        // draft-koch-openpgp-webkey-service §3.1: "Joe.Doe@Example.ORG".
        let urls = wkd_urls("Joe.Doe@Example.ORG").unwrap();
        assert_eq!(
            urls.advanced,
            "https://openpgpkey.example.org/.well-known/openpgpkey/example.org/hu/iy9q119eutrkn8s1mk4r39qejnbu3n5q?l=Joe.Doe"
        );
        assert_eq!(
            urls.direct,
            "https://example.org/.well-known/openpgpkey/hu/iy9q119eutrkn8s1mk4r39qejnbu3n5q?l=Joe.Doe"
        );
        assert_eq!(zbase32(&[]), "");
        assert_eq!(zbase32(&[0xF0]), "6y");
        assert!(wkd_urls("no-at-sign").is_err());
        assert!(wkd_urls("a@exa mple.org").is_err());
        assert_eq!(
            keys_openpgp_org_url("a+b@example.org").unwrap(),
            "https://keys.openpgp.org/vks/v1/by-email/a%2Bb%40example.org"
        );
    }

    #[test]
    fn wkd_advanced_then_direct_then_keys_openpgp_org() {
        let urls = wkd_urls("alice@example.org").unwrap();
        let (keys, _) = key::parse_keys(&fixture("alice.pub.asc"));
        let binary = key::to_binary(&keys[0].public()).unwrap();
        // Advanced host fails, the direct method has the key; VKS has it
        // too (deduplicated).
        let http = FakeHttp::default()
            .fail(&urls.advanced)
            .answer(&urls.direct, 200, &binary)
            .answer(
                &keys_openpgp_org_url("alice@example.org").unwrap(),
                200,
                &fixture("alice.pub.asc"),
            );
        let result = block_on(lookup(&http, "alice@example.org", &BOTH, NOW)).unwrap();
        assert!(result.failures.is_empty(), "{:?}", result.failures);
        assert_eq!(result.candidates.len(), 1);
        let candidate = &result.candidates[0];
        assert_eq!(candidate.source, KeyLookupSource::Wkd);
        assert_eq!(
            candidate.user_ids,
            vec!["Alice Example <alice@example.org>"]
        );
        assert!(
            candidate
                .armored
                .starts_with("-----BEGIN PGP PUBLIC KEY BLOCK-----")
        );
        assert_eq!(http.requested.lock().unwrap().len(), 3);

        // Import it and mark it as known.
        let store = nail_store::Store::open_in_memory().unwrap();
        let secrets = MemorySecretStore::new();
        let cache = UnlockCache::new();
        let ring = Keyring::new(&store, &secrets, &cache);
        let key = import_candidate(&ring, candidate).unwrap();
        assert_eq!(key.source, KeySource::Wkd);
        let mut candidates = result.candidates.clone();
        mark_known(&ring, &mut candidates).unwrap();
        assert_eq!(candidates[0].known.as_ref(), Some(&key.id));
    }

    #[test]
    fn not_found_is_not_a_failure_but_errors_are() {
        let http = FakeHttp::default();
        let result = block_on(lookup(&http, "nobody@example.org", &BOTH, NOW)).unwrap();
        assert!(result.candidates.is_empty() && result.failures.is_empty());

        let urls = wkd_urls("nobody@example.org").unwrap();
        let http = FakeHttp::default()
            .fail(&urls.advanced)
            .fail(&urls.direct)
            .answer(
                &keys_openpgp_org_url("nobody@example.org").unwrap(),
                500,
                b"",
            );
        let result = block_on(lookup(&http, "nobody@example.org", &BOTH, NOW)).unwrap();
        assert_eq!(result.failures.len(), 2);
        assert_eq!(result.failures[0].source, KeyLookupSource::Wkd);
        assert_eq!(result.failures[0].code, ErrorCode::Network);
        assert_eq!(result.failures[1].source, KeyLookupSource::KeysOpenpgpOrg);

        let http =
            FakeHttp::default().answer(&urls.advanced, 200, &vec![0u8; MAX_RESPONSE_BYTES + 1]);
        let result = block_on(lookup(
            &http,
            "nobody@example.org",
            &[KeyLookupSource::Wkd],
            NOW,
        ))
        .unwrap();
        // Too large at the advanced host: the direct method is tried (404).
        assert!(result.failures.is_empty());
    }

    #[test]
    fn keys_for_other_addresses_and_secret_keys_are_filtered() {
        let urls = wkd_urls("mallory@example.org").unwrap();
        // The directory serves alice's key for mallory: rejected.
        let http = FakeHttp::default().answer(&urls.advanced, 200, &fixture("alice.pub.asc"));
        let result = block_on(lookup(
            &http,
            "mallory@example.org",
            &[KeyLookupSource::Wkd],
            NOW,
        ))
        .unwrap();
        assert!(result.candidates.is_empty());

        // A secret key served by mistake yields only its public part.
        let urls = wkd_urls("bob@example.net").unwrap();
        let http = FakeHttp::default().answer(&urls.advanced, 200, &fixture("bob.sec.asc"));
        let result = block_on(lookup(
            &http,
            "bob@example.net",
            &[KeyLookupSource::Wkd],
            NOW,
        ))
        .unwrap();
        assert_eq!(result.candidates.len(), 1);
        assert!(result.candidates[0].armored.contains("PUBLIC KEY BLOCK"));
        assert!(!result.candidates[0].armored.contains("PRIVATE"));
    }

    #[test]
    fn valid_keys_sort_before_expired_ones() {
        let mut data = fixture("dave.pub.asc");
        data.extend_from_slice(&fixture("alice.pub.asc"));
        let mut both = candidates_from(&data, "dave@example.org", KeyLookupSource::Wkd, NOW);
        assert_eq!(both.len(), 1, "only dave's key is for dave");
        assert!(!both.remove(0).0, "dave's key is expired");
    }
}
