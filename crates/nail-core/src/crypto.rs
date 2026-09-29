//! End-to-end encryption in the core (M2a WP35, ADR 0034): OpenPGP
//! (`nail-pgp`, ADR 0030) and S/MIME (`nail-smime`, ADR 0031) wired into
//! reading, sending and arriving mail.
//!
//! * **Reading** ([`Service::message_body`]): S/MIME and PGP/MIME (and
//!   inline PGP) are decrypted and verified; the reader gets the decrypted
//!   content with its [`SecurityStatus`]. Decrypted content is never
//!   written anywhere: only the status goes into the `message_security`
//!   cache, and the search index gets decrypted text only for accounts that
//!   opted in ([`SecuritySettings::index_decrypted`]). Sender
//!   authentication (`Authentication-Results` of the account's trusted
//!   servers) and link / attachment warnings are added to every body.
//! * **Sending** ([`secure_outgoing`]): the `Autocrypt` header, then
//!   signing and / or encryption with the chosen technology. Sending never
//!   falls back to plain text: a missing key is `no_key`, a locked one
//!   `locked`. Bcc recipients get their own encrypted copy, so the other
//!   recipients never learn about them from the key list.
//! * **Arriving mail** ([`Service::secure_arrivals`], run with the arrival
//!   rules): Autocrypt peer state from the `Autocrypt` header and, for
//!   encrypted mail, from the gossip inside; decrypted indexing for
//!   accounts that opted in.

use std::sync::Arc;
use std::time::Duration;

use nail_api::engine::{BoxFuture, EngineError, Envelope};
use nail_api::security::{
    AutocryptPreference, CryptoPreference, CryptoTechnology, IdentityCrypto, SecurityStatus,
    SendCrypto,
};
use nail_api::{
    AccountId, ApiError, ErrorCode, Event, FolderRole, Identity, MessageBody, MessageId,
};
use nail_compose::OutgoingMessage;
use nail_pgp::discovery::{HttpClient, HttpResponse, MAX_RESPONSE_BYTES};
use nail_search::IndexPolicy;
use nail_smime::mime::SmimeKind;
use nail_store::{MessageSecurityRecord, StoredKey};

use crate::engine::unix_now;
use crate::error::{CoreError, Result};
use crate::service::{Service, ServiceOptions};

/// Recent inbox messages looked at to suggest the `authserv-id`.
const LEARN_AUTHSERV_SAMPLE: u32 = 20;

/// Share of those messages whose topmost `Authentication-Results` must
/// name the same server.
const LEARN_AUTHSERV_SHARE: f64 = 0.8;

/// State of end-to-end encryption, one per service.
pub(crate) struct CryptoState {
    /// Unlocked OpenPGP passphrases (memory only).
    pub(crate) pgp_unlock: nail_pgp::keyring::UnlockCache,
    /// Unlocked S/MIME private keys (memory only).
    pub(crate) smime_unlock: nail_smime::keyring::UnlockCache,
    /// Trust anchors for S/MIME chains.
    pub(crate) anchors: Arc<dyn nail_smime::TrustAnchors>,
    /// HTTPS for WKD / keys.openpgp.org.
    pub(crate) http: Arc<dyn HttpClient>,
}

impl CryptoState {
    pub(crate) fn new(options: &ServiceOptions) -> Result<Self> {
        let anchors = match &options.trust_anchors {
            Some(anchors) => Arc::clone(anchors),
            None => default_trust_anchors(),
        };
        let http = match &options.key_lookup {
            Some(http) => Arc::clone(http),
            None => Arc::new(KeyLookupHttp::new()?) as Arc<dyn HttpClient>,
        };
        Ok(Self {
            pgp_unlock: nail_pgp::keyring::UnlockCache::new(),
            smime_unlock: nail_smime::keyring::UnlockCache::new(),
            anchors,
            http,
        })
    }
}

#[cfg(windows)]
fn default_trust_anchors() -> Arc<dyn nail_smime::TrustAnchors> {
    Arc::new(nail_smime::windows::WindowsTrustAnchors)
}

#[cfg(not(windows))]
fn default_trust_anchors() -> Arc<dyn nail_smime::TrustAnchors> {
    Arc::new(nail_smime::FixedTrustAnchors::new(Vec::new()))
}

/// [`HttpClient`] on reqwest: rustls with the platform verifier (ADR
/// 0014), HTTPS only, at most three redirects (HTTPS only), short
/// timeouts, reading stops after [`MAX_RESPONSE_BYTES`].
struct KeyLookupHttp(reqwest::Client);

impl KeyLookupHttp {
    fn new() -> Result<Self> {
        let tls = nail_auth::tls_config()
            .map_err(|e| ApiError::internal(format!("TLS for key lookups: {e}")))?;
        let client = nail_auth::http_client_builder(tls)
            .https_only(true)
            .redirect(reqwest::redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= 3 || attempt.url().scheme() != "https" {
                    attempt.stop()
                } else {
                    attempt.follow()
                }
            }))
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .build()
            .map_err(|e| ApiError::internal(format!("HTTP client for key lookups: {e}")))?;
        Ok(Self(client))
    }
}

fn lookup_error(err: &reqwest::Error) -> EngineError {
    let text = format!("{err:?}").to_ascii_lowercase();
    let code = if text.contains("certificate") || text.contains("tls") {
        ErrorCode::Tls
    } else {
        ErrorCode::Network
    };
    EngineError::temporary(code, err.to_string())
}

impl HttpClient for KeyLookupHttp {
    fn get<'a>(
        &'a self,
        url: &'a str,
    ) -> BoxFuture<'a, std::result::Result<HttpResponse, EngineError>> {
        Box::pin(async move {
            if !url.starts_with("https://") {
                return Err(EngineError::permanent(
                    ErrorCode::InvalidRequest,
                    format!("key lookups use HTTPS only: {url}"),
                ));
            }
            let mut response = self.0.get(url).send().await.map_err(|e| lookup_error(&e))?;
            let status = response.status().as_u16();
            let mut body = Vec::new();
            while let Some(chunk) = response.chunk().await.map_err(|e| lookup_error(&e))? {
                body.extend_from_slice(&chunk);
                if body.len() > MAX_RESPONSE_BYTES {
                    break;
                }
            }
            Ok(HttpResponse { status, body })
        })
    }
}

// ---------------------------------------------------------------------------
// Keyrings

impl Service {
    /// The OpenPGP keyring (blocking calls).
    pub(crate) fn pgp(&self) -> nail_pgp::keyring::Keyring<'_> {
        nail_pgp::keyring::Keyring::new(
            &self.inner.store,
            self.inner.secrets.as_ref(),
            &self.inner.crypto.pgp_unlock,
        )
    }

    /// The S/MIME keyring (blocking calls).
    pub(crate) fn smime(&self) -> nail_smime::keyring::Keyring<'_> {
        nail_smime::keyring::Keyring::new(
            &self.inner.store,
            self.inner.secrets.as_ref(),
            self.inner.crypto.anchors.as_ref(),
            &self.inner.crypto.smime_unlock,
        )
    }

    /// Runs blocking keyring work on the blocking pool.
    pub(crate) async fn blocking<T: Send + 'static>(
        &self,
        work: impl FnOnce(&Service) -> Result<T> + Send + 'static,
    ) -> Result<T> {
        let service = self.clone();
        tokio::task::spawn_blocking(move || work(&service)).await?
    }
}

// ---------------------------------------------------------------------------
// Raw MIME helpers

/// Header fields of a message (each with its folded lines and line end)
/// and the body.
pub(crate) fn split_header(raw: &[u8]) -> (Vec<(String, Vec<u8>)>, &[u8]) {
    let mut fields: Vec<(String, Vec<u8>)> = Vec::new();
    let mut pos = 0;
    while pos < raw.len() {
        let end = raw[pos..]
            .iter()
            .position(|&b| b == b'\n')
            .map_or(raw.len(), |i| pos + i + 1);
        let line = &raw[pos..end];
        let content = line
            .strip_suffix(b"\n")
            .map_or(line, |l| l.strip_suffix(b"\r").unwrap_or(l));
        if content.is_empty() {
            return (fields, &raw[end..]);
        }
        if matches!(line[0], b' ' | b'\t') {
            if let Some((_, last)) = fields.last_mut() {
                last.extend_from_slice(line);
            }
        } else {
            let name = content
                .split(|&b| b == b':')
                .next()
                .map(|n| String::from_utf8_lossy(n).trim().to_owned())
                .unwrap_or_default();
            fields.push((name, line.to_vec()));
        }
        pos = end;
    }
    (fields, &[])
}

fn is_content_field(name: &str) -> bool {
    name.to_ascii_lowercase().starts_with("content-")
}

/// Writes a field, ending it with CRLF.
fn push_field(out: &mut Vec<u8>, field: &[u8]) {
    let trimmed = field
        .strip_suffix(b"\n")
        .map_or(field, |l| l.strip_suffix(b"\r").unwrap_or(l));
    out.extend_from_slice(trimmed);
    out.extend_from_slice(b"\r\n");
}

/// The message's non-content header fields followed by `entity` (a MIME
/// entity: `Content-*` fields and body), with `MIME-Version: 1.0`.
pub(crate) fn with_outer_headers(raw: &[u8], entity: &[u8]) -> Vec<u8> {
    let (fields, _) = split_header(raw);
    let mut out = Vec::with_capacity(raw.len().min(64 * 1024) + entity.len());
    for (name, field) in &fields {
        if !is_content_field(name) && !name.eq_ignore_ascii_case("MIME-Version") {
            push_field(&mut out, field);
        }
    }
    out.extend_from_slice(b"MIME-Version: 1.0\r\n");
    out.extend_from_slice(entity);
    out
}

/// The message's content as a MIME entity (its `Content-*` fields and
/// body; `text/plain` when it declares none).
pub(crate) fn content_entity(raw: &[u8]) -> Vec<u8> {
    let (fields, body) = split_header(raw);
    let mut out = Vec::with_capacity(body.len() + 256);
    let mut typed = false;
    for (name, field) in &fields {
        if is_content_field(name) {
            typed |= name.eq_ignore_ascii_case("Content-Type");
            push_field(&mut out, field);
        }
    }
    if !typed {
        out.extend_from_slice(b"Content-Type: text/plain; charset=us-ascii\r\n");
    }
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(body);
    out
}

/// A cheap first look whether a raw message is end-to-end encrypted:
/// PGP/MIME, S/MIME enveloped data, or an inline PGP message at the start
/// of a line. Used to pick messages for decrypted indexing and rules; it
/// can miss unusual wrappings, so access control never relies on it (see
/// [`Service::message_is_encrypted`]).
pub(crate) fn is_encrypted_raw(raw: &[u8]) -> bool {
    if encrypted_content_type(raw) || has_inline_pgp_message(raw) {
        return true;
    }
    matches!(
        nail_smime::mime::classify(raw),
        SmimeKind::EnvelopedData | SmimeKind::AuthEnvelopedData
    )
}

/// An armored OpenPGP message at the start of a line anywhere in the raw
/// message.
fn has_inline_pgp_message(raw: &[u8]) -> bool {
    const ARMOR: &[u8] = b"-----BEGIN PGP MESSAGE-----";
    raw.windows(ARMOR.len())
        .enumerate()
        .any(|(i, w)| w == ARMOR && (i == 0 || raw[i - 1] == b'\n'))
}

/// The top-level `Content-Type` is an encrypted type.
fn encrypted_content_type(raw: &[u8]) -> bool {
    let header_end = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n"))
        .unwrap_or(raw.len());
    let headers = String::from_utf8_lossy(&raw[..header_end]).to_ascii_lowercase();
    // Unfold continuation lines, then find the Content-Type header.
    let unfolded = headers
        .replace("\r\n", "\n")
        .replace("\n ", " ")
        .replace("\n\t", " ");
    unfolded.lines().any(|line| {
        line.strip_prefix("content-type:").is_some_and(|value| {
            let value = value.trim();
            value.starts_with("multipart/encrypted")
                || value.starts_with("application/pkcs7-mime")
                || value.starts_with("application/x-pkcs7-mime")
        })
    })
}

fn first_from(raw: &[u8]) -> Option<String> {
    nail_mime::parse_message(raw)
        .headers
        .from
        .first()
        .map(|a| a.email.to_ascii_lowercase())
}

// ---------------------------------------------------------------------------
// Reading

/// A message as the reader sees it.
#[derive(Debug, Default)]
pub(crate) struct Opened {
    /// End-to-end status (`None`: neither signed nor encrypted).
    pub status: Option<SecurityStatus>,
    /// The message to show instead of the raw one (decrypted, or the
    /// signed content unwrapped). Never stored.
    pub display: Option<Vec<u8>>,
    /// `Autocrypt-Gossip` values from inside an encrypted part.
    pub gossip: Vec<String>,
    /// Keys the reading stored (S/MIME signer certificates).
    pub new_keys: bool,
}

/// Decrypts and verifies a raw message (blocking). Failures of the crypto
/// libraries become a status, never an error: the reader always shows
/// something.
pub(crate) fn open_message(service: &Service, raw: &[u8]) -> Opened {
    let kind = nail_smime::mime::classify(raw);
    if !matches!(kind, SmimeKind::None | SmimeKind::CertsOnly) {
        let smime = service.smime();
        let from = first_from(raw);
        match smime.read_message(raw, from.as_deref()) {
            Ok(Some(outcome)) => {
                let new_keys = match smime.remember_certificates(&outcome) {
                    Ok(keys) => !keys.is_empty(),
                    Err(err) => {
                        tracing::warn!(%err, "cannot remember S/MIME certificates");
                        false
                    }
                };
                return Opened {
                    display: Some(with_outer_headers(raw, &outcome.content)),
                    status: Some(outcome.status),
                    gossip: Vec::new(),
                    new_keys,
                };
            }
            Ok(None) => {}
            Err(err) => tracing::warn!(%err, "cannot read an S/MIME message"),
        }
    }
    match nail_pgp::read::read_message(&service.pgp(), raw) {
        Ok(outcome) => Opened {
            status: outcome.status,
            display: outcome.display,
            gossip: outcome.gossip,
            new_keys: false,
        },
        Err(err) => {
            tracing::warn!(%err, "cannot read an OpenPGP message");
            Opened::default()
        }
    }
}

impl Service {
    /// `GetMessage`: the body as the reader shows it (see the module docs).
    pub(crate) async fn message_body(&self, id: MessageId) -> Result<MessageBody> {
        self.inner.vault.check()?;
        let (body, keys_changed) = self
            .blocking(move |service| {
                let store = service.store();
                let stored = store.get_message(&id)?;
                let hash = stored.blob_hash.ok_or_else(|| {
                    CoreError::from(ApiError::new(
                        ErrorCode::Unavailable,
                        format!("the body of message {id} is not downloaded yet"),
                    ))
                })?;
                let raw = service.blobs().get(&hash)?;
                let opened = open_message(service, &raw);
                let display = opened.display.as_deref().unwrap_or(&raw);
                let mut body = nail_mime::parse_message(display).into_body(id.clone());
                body.security = opened.status.clone();
                let trusted = service.trusted_authserv_ids(&stored.summary.account_id);
                body.auth_results = nail_mime::auth_results::auth_results_of_raw(&raw, &trusted);
                body.warnings = nail_mime::links::message_warnings(&body);
                // Only the findings are cached, never decrypted content.
                let record = MessageSecurityRecord {
                    message_id: id.clone(),
                    status: body.security.clone(),
                    auth_results: body.auth_results.clone(),
                    checked_at: unix_now(),
                };
                if let Err(err) = store.put_message_security(&record) {
                    tracing::debug!(%err, "cannot cache the message's security findings");
                }
                let mut keys_changed = opened.new_keys;
                if !opened.gossip.is_empty() {
                    keys_changed |=
                        service.apply_autocrypt(&stored.summary.account_id, &raw, &opened.gossip);
                }
                Ok((body, keys_changed))
            })
            .await?;
        if keys_changed {
            self.broadcast(Event::KeysChanged);
        }
        Ok(body)
    }

    /// A message's content as the composer quotes and forwards it: the
    /// decrypted message for encrypted mail, else the raw one (blocking).
    pub(crate) fn readable_raw(&self, raw: Vec<u8>) -> (Vec<u8>, Option<SecurityStatus>) {
        let opened = open_message(self, &raw);
        (opened.display.unwrap_or(raw), opened.status)
    }

    /// Whether the reader treats a message as end-to-end encrypted: its
    /// security status reports encryption (blocking). This is what the MCP
    /// `encrypted` grant is enforced with, so it comes from the reader
    /// itself ([`open_message`], the same code `GetMessage` runs), never
    /// from a byte heuristic: every wrapping the reader decrypts counts
    /// (inline PGP in an encoded or quoted text part, relabelled or signed
    /// S/MIME envelopes). A cached status or the cheap [`is_encrypted_raw`]
    /// saying "encrypted" answers early (they can only add gating, never
    /// remove it). A message whose body is not downloaded has no content to
    /// reveal: `false`.
    pub(crate) fn message_is_encrypted(&self, id: &MessageId) -> Result<bool> {
        let store = self.store();
        if store
            .message_security(id)?
            .and_then(|r| r.status)
            .is_some_and(|s| s.is_encrypted())
        {
            return Ok(true);
        }
        let Some(hash) = store.get_message(id)?.blob_hash else {
            return Ok(false);
        };
        let raw = self.blobs().get(&hash)?;
        if is_encrypted_raw(&raw) {
            return Ok(true);
        }
        Ok(open_message(self, &raw)
            .status
            .is_some_and(|s| s.is_encrypted()))
    }

    /// The `authserv-id`s whose results count for `account`: only the
    /// configured ones (`authserv-id` in `accounts.kdl`). An id learned from
    /// mail is never trusted by itself (a sender can write any
    /// `Authentication-Results` header when the receiving server adds
    /// none); [`Self::suggest_authserv_id`] proposes one for the user to
    /// confirm.
    pub(crate) fn trusted_authserv_ids(&self, account: &AccountId) -> Vec<String> {
        self.inner
            .accounts
            .config()
            .account(account)
            .map(|config| config.authserv_ids.clone())
            .unwrap_or_default()
    }

    /// `SuggestAuthservId`: the id on top of most of the account's recent
    /// inbox mail and bound to the server that received it
    /// ([`nail_mime::auth_results::learn_authserv_id`]); computed on
    /// request, never stored or trusted (blocking).
    pub(crate) fn suggest_authserv_id(&self, account: &AccountId) -> Result<Option<String>> {
        let store = self.store();
        let Some(inbox) = store.folder_with_role(account, FolderRole::Inbox)? else {
            return Ok(None);
        };
        let page = store.list_messages(&inbox, 0, LEARN_AUTHSERV_SAMPLE)?;
        let mut headers = Vec::new();
        for summary in &page.messages {
            let message = store.get_message(&summary.id)?;
            if let Some(raw) = message.blob_hash.and_then(|h| self.blobs().get(&h).ok()) {
                headers.push(nail_mime::header_fields(&raw));
            }
        }
        Ok(nail_mime::auth_results::learn_authserv_id(
            headers.iter().map(Vec::as_slice),
            LEARN_AUTHSERV_SHARE,
        ))
    }

    /// Applies a received message to the Autocrypt state of `account`
    /// (blocking); `true` when keys were stored.
    pub(crate) fn apply_autocrypt(
        &self,
        account: &AccountId,
        raw: &[u8],
        gossip: &[String],
    ) -> bool {
        if !self.autocrypt_enabled(account) {
            return false;
        }
        let incoming = nail_pgp::autocrypt::IncomingMessage::from_raw(raw);
        match nail_pgp::autocrypt::process_incoming(&self.pgp(), account, &incoming, gossip) {
            Ok(update) => !update.keys.is_empty(),
            Err(err) => {
                tracing::debug!(%err, "cannot apply the Autocrypt header");
                false
            }
        }
    }

    /// Whether any identity of the account uses Autocrypt (the default).
    fn autocrypt_enabled(&self, account: &AccountId) -> bool {
        match self.inner.accounts.config().account(account) {
            Some(config) => config
                .identities
                .iter()
                .any(|i| i.crypto.as_ref().is_none_or(|c| c.autocrypt)),
            // Local and demo accounts without configuration.
            None => true,
        }
    }

    /// Security work for newly arrived mail (run with the arrival rules):
    /// Autocrypt headers and gossip, decrypted indexing for accounts that
    /// opted in.
    pub(crate) async fn secure_arrivals(&self, ids: Vec<MessageId>) {
        let settings = match crate::security::security_settings(self).await {
            Ok(settings) => settings,
            Err(err) => {
                tracing::warn!(%err, "cannot read the security settings");
                return;
            }
        };
        let result = self
            .blocking(move |service| {
                let policy = IndexPolicy::from_settings(&settings);
                let index = service.inner.search.get();
                let mut keys_changed = false;
                let mut docs = Vec::new();
                for id in &ids {
                    let Ok(stored) = service.store().get_message(id) else {
                        continue;
                    };
                    let Some(raw) = stored.blob_hash.and_then(|h| service.blobs().get(&h).ok())
                    else {
                        continue;
                    };
                    let account = stored.summary.account_id.clone();
                    keys_changed |= service.apply_autocrypt(&account, &raw, &[]);
                    if !is_encrypted_raw(&raw) {
                        continue;
                    }
                    let opened = open_message(service, &raw);
                    keys_changed |= opened.new_keys;
                    if !opened.gossip.is_empty() {
                        keys_changed |= service.apply_autocrypt(&account, &raw, &opened.gossip);
                    }
                    let outer = nail_mime::parse_message(&raw);
                    let inner = opened.display.as_deref().map(nail_mime::parse_message);
                    docs.push(policy.document(id.clone(), account, &outer, inner.as_ref()));
                }
                if let Err(err) = index.add(&docs) {
                    tracing::warn!(%err, "cannot index encrypted messages");
                }
                Ok(keys_changed)
            })
            .await;
        match result {
            Ok(true) => self.broadcast(Event::KeysChanged),
            Ok(false) => {}
            Err(err) => tracing::warn!(%err, "cannot process the security of new mail"),
        }
    }

    /// Reindexes the encrypted messages of `accounts` under `policy`
    /// (decrypted where the account opted in, else from the outer message
    /// only). Blocking.
    pub(crate) fn reindex_encrypted(
        &self,
        accounts: &[AccountId],
        policy: &IndexPolicy,
    ) -> Result<usize> {
        if accounts.is_empty() {
            return Ok(0);
        }
        let index = self.inner.search.get();
        for account in accounts {
            index.remove_decrypted(account)?;
        }
        let ids = self
            .store()
            .filter_message_ids(&nail_store::MessageFilter::AccountIn(accounts.to_vec()))?;
        let mut count = 0;
        for chunk in ids.chunks(100) {
            let mut docs = Vec::new();
            for id in chunk {
                let Ok(stored) = self.store().get_message(id) else {
                    continue;
                };
                let Some(raw) = stored.blob_hash.and_then(|h| self.blobs().get(&h).ok()) else {
                    continue;
                };
                if !is_encrypted_raw(&raw) {
                    continue;
                }
                let outer = nail_mime::parse_message(&raw);
                let account = stored.summary.account_id.clone();
                let inner = if policy.indexes_decrypted(&account) {
                    open_message(self, &raw)
                        .display
                        .map(|d| nail_mime::parse_message(&d))
                } else {
                    None
                };
                docs.push(policy.document(id.clone(), account, &outer, inner.as_ref()));
            }
            count += docs.len();
            index.add(&docs)?;
        }
        index.commit()?;
        Ok(count)
    }
}

// ---------------------------------------------------------------------------
// Sending

/// The sign / encrypt choice of a message: the request's, else the
/// draft's, else the identity's defaults (ADR 0029).
pub(crate) fn effective_crypto(
    request: Option<SendCrypto>,
    draft: Option<&SendCrypto>,
    identity: &Identity,
) -> SendCrypto {
    let defaults = identity.crypto.clone().unwrap_or_default();
    request.or_else(|| draft.cloned()).unwrap_or(SendCrypto {
        sign: defaults.sign,
        encrypt: defaults.encrypt,
        technology: None,
        protect_headers: None,
    })
}

fn normalized(emails: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for email in emails {
        let email = email.trim().to_ascii_lowercase();
        if !email.is_empty() && !out.contains(&email) {
            out.push(email);
        }
    }
    out
}

/// The own S/MIME certificate of an identity (the configured one, else any
/// own certificate for the address).
fn smime_own(
    service: &Service,
    email: &str,
    fingerprint: Option<&str>,
) -> Result<Option<StoredKey>> {
    Ok(service
        .store()
        .list_keys(Some(CryptoTechnology::Smime), true)?
        .into_iter()
        .filter(|k| match fingerprint {
            Some(fp) => k.data.fingerprint.eq_ignore_ascii_case(fp),
            None => k.data.emails.iter().any(|e| e.eq_ignore_ascii_case(email)),
        })
        .max_by_key(|k| k.data.created_at))
}

/// Which own keys an identity has (blocking).
pub(crate) struct OwnKeys {
    /// An own OpenPGP key that can sign and encrypt.
    pub openpgp: bool,
    /// An own S/MIME certificate.
    pub smime: bool,
}

pub(crate) fn own_keys(service: &Service, identity: &Identity) -> Result<OwnKeys> {
    let crypto = identity.crypto.clone().unwrap_or_default();
    let pgp = service.pgp();
    let openpgp = match pgp.own_key_for(&identity.email, crypto.openpgp_key.as_deref()) {
        Ok(stored) => {
            pgp.encryption_keys(std::slice::from_ref(&identity.email.to_ascii_lowercase()))
                .is_ok()
                || stored.data.can_encrypt
        }
        Err(_) => false,
    };
    let smime = smime_own(
        service,
        &identity.email,
        crypto.smime_certificate.as_deref(),
    )?
    .is_some();
    Ok(OwnKeys { openpgp, smime })
}

/// The technology for a message: the request's, the identity's
/// preference, else what the own keys and every recipient support
/// (OpenPGP first).
pub(crate) fn choose_technology(
    service: &Service,
    identity: &Identity,
    crypto: &SendCrypto,
    recipients: &[String],
) -> Result<CryptoTechnology> {
    if let Some(technology) = crypto.technology {
        return Ok(technology);
    }
    let settings = identity.crypto.clone().unwrap_or_default();
    match settings.technology {
        CryptoPreference::OpenPgp => return Ok(CryptoTechnology::OpenPgp),
        CryptoPreference::Smime => return Ok(CryptoTechnology::Smime),
        CryptoPreference::Auto => {}
    }
    let own = own_keys(service, identity)?;
    if crypto.encrypt {
        if own.openpgp && service.pgp().encryption_keys(recipients).is_ok() {
            return Ok(CryptoTechnology::OpenPgp);
        }
        let refs: Vec<&str> = recipients.iter().map(String::as_str).collect();
        if own.smime && service.smime().encryption_certificates(&refs).is_ok() {
            return Ok(CryptoTechnology::Smime);
        }
    }
    Ok(if !own.openpgp && own.smime {
        CryptoTechnology::Smime
    } else {
        CryptoTechnology::OpenPgp
    })
}

/// The `To` / `Cc` recipients of a built message and the rest of the
/// envelope (Bcc, also the identity's automatic Bcc).
fn recipient_sets(draft: &nail_api::Draft, envelope: &Envelope) -> (Vec<String>, Vec<String>) {
    let visible = normalized(draft.to.iter().chain(&draft.cc).map(|a| a.email.clone()));
    let hidden = normalized(
        envelope
            .to
            .iter()
            .filter(|r| !visible.contains(&r.trim().to_ascii_lowercase()))
            .cloned(),
    );
    (visible, hidden)
}

/// Checks that a message can be protected as asked, without building it:
/// the keys exist and the signing key is unlocked (blocking). Returns the
/// technology to use.
pub(crate) fn check_send(
    service: &Service,
    identity: &Identity,
    draft: &nail_api::Draft,
    crypto: &SendCrypto,
) -> Result<CryptoTechnology> {
    let settings = identity.crypto.clone().unwrap_or_default();
    let recipients = normalized(
        draft
            .to
            .iter()
            .chain(&draft.cc)
            .chain(&draft.bcc)
            .map(|a| a.email.clone()),
    );
    let technology = choose_technology(service, identity, crypto, &recipients)?;
    match technology {
        CryptoTechnology::OpenPgp => {
            let pgp = service.pgp();
            let own = pgp.own_key_for(&identity.email, settings.openpgp_key.as_deref())?;
            if crypto.sign {
                pgp.unlocked_key(&own)?;
            }
            if crypto.encrypt {
                pgp.encryption_keys(&recipients)?;
            }
        }
        CryptoTechnology::Smime => {
            let smime = service.smime();
            if crypto.sign {
                smime.signer(&identity.email, settings.smime_certificate.as_deref())?;
            }
            if crypto.encrypt {
                let refs: Vec<&str> = recipients.iter().map(String::as_str).collect();
                smime.encryption_certificates(&refs)?;
                smime_self_certificate(service, identity, &settings)?;
            }
        }
    }
    Ok(technology)
}

/// The sender's own certificate to keep an encrypted copy readable.
fn smime_self_certificate(
    service: &Service,
    identity: &Identity,
    settings: &IdentityCrypto,
) -> Result<nail_smime::Cert> {
    let own = smime_own(
        service,
        &identity.email,
        settings.smime_certificate.as_deref(),
    )?
    .ok_or_else(|| {
        nail_smime::Error::NoKey(format!("no own S/MIME certificate for {}", identity.email))
    })?;
    Ok(nail_smime::Cert::from_der(&own.data.public_data)?)
}

/// Adds the `Autocrypt` header and signs / encrypts a built message
/// (blocking). The first message is the one for the `To` / `Cc`
/// recipients (and the Sent copy); each Bcc recipient of an encrypted
/// message gets its own copy after it.
pub(crate) fn secure_outgoing(
    service: &Service,
    identity: &Identity,
    draft: &nail_api::Draft,
    message: OutgoingMessage,
    crypto: &SendCrypto,
) -> Result<Vec<OutgoingMessage>> {
    let settings = identity.crypto.clone().unwrap_or_default();
    let now = unix_now();
    let pgp = service.pgp();
    let own_pgp = pgp
        .own_key_for(&identity.email, settings.openpgp_key.as_deref())
        .ok();
    let mut raw = message.raw;
    if settings.autocrypt
        && let Some(own) = &own_pgp
        && let Ok(public) = pgp.public_key(own)
        && let Ok(header) = nail_pgp::autocrypt::AutocryptHeader::for_key(
            &public,
            &identity.email,
            settings.prefer_encrypt,
            now,
        )
    {
        raw = nail_pgp::compose::with_header(&raw, "Autocrypt", &header.to_value(false));
    }
    if !crypto.is_active() {
        return Ok(vec![OutgoingMessage { raw, ..message }]);
    }
    let (visible, hidden) = recipient_sets(draft, &message.envelope);
    let mut all = visible.clone();
    all.extend(hidden.iter().cloned());
    let technology = choose_technology(service, identity, crypto, &all)?;
    let protect_headers = crypto.protect_headers.unwrap_or(settings.protect_headers);
    // (recipients to encrypt to, envelope recipients) per copy.
    let mut copies: Vec<(Vec<String>, Vec<String>)> = Vec::new();
    if crypto.encrypt && !hidden.is_empty() {
        let main_envelope: Vec<String> = message
            .envelope
            .to
            .iter()
            .filter(|r| !hidden.contains(&r.trim().to_ascii_lowercase()))
            .cloned()
            .collect();
        if !main_envelope.is_empty() {
            copies.push((visible.clone(), main_envelope));
        }
        for bcc in &hidden {
            let mut keys = visible.clone();
            keys.push(bcc.clone());
            copies.push((keys, vec![bcc.clone()]));
        }
    } else {
        copies.push((all.clone(), message.envelope.to.clone()));
    }
    let mut out = Vec::with_capacity(copies.len());
    match technology {
        CryptoTechnology::OpenPgp => {
            let own = own_pgp.ok_or_else(|| {
                nail_pgp::Error::NoKey(format!("no own OpenPGP key for {}", identity.email))
            })?;
            let signer = if crypto.sign {
                Some(pgp.unlocked_key(&own)?)
            } else {
                None
            };
            let own_public = pgp.public_key(&own)?;
            for (keys_for, envelope_to) in copies {
                let mut recipients = Vec::new();
                let mut gossip = Vec::new();
                if crypto.encrypt {
                    let keys = pgp.encryption_keys(&keys_for)?;
                    if settings.autocrypt && keys_for.len() > 1 {
                        for (email, key) in keys_for.iter().zip(&keys) {
                            if let Ok(header) = nail_pgp::autocrypt::AutocryptHeader::for_key(
                                key,
                                email,
                                AutocryptPreference::NoPreference,
                                now,
                            ) {
                                gossip.push(header.to_value(true));
                            }
                        }
                    }
                    recipients = keys;
                    recipients.push(own_public.clone());
                }
                let protected = nail_pgp::compose::protect_message(
                    &raw,
                    &nail_pgp::compose::Protection {
                        signer: signer.as_ref(),
                        recipients: &recipients,
                        protect_headers,
                        gossip: &gossip,
                        now,
                    },
                )?;
                out.push(OutgoingMessage {
                    envelope: Envelope {
                        from: message.envelope.from.clone(),
                        to: envelope_to,
                    },
                    raw: protected,
                    message_id: message.message_id.clone(),
                });
            }
        }
        CryptoTechnology::Smime => {
            let smime = service.smime();
            let mut entity = content_entity(&raw);
            if crypto.sign {
                let own = smime.signer(&identity.email, settings.smime_certificate.as_deref())?;
                entity = nail_smime::mime::sign_entity(
                    &entity,
                    &own.signer(),
                    nail_smime::SignMode::Detached,
                    now,
                )?;
            }
            let own_cert = if crypto.encrypt {
                Some(smime_self_certificate(service, identity, &settings)?)
            } else {
                None
            };
            for (keys_for, envelope_to) in copies {
                let protected = match &own_cert {
                    Some(own_cert) => {
                        let refs: Vec<&str> = keys_for.iter().map(String::as_str).collect();
                        let mut certs = smime.encryption_certificates(&refs)?;
                        if !certs.contains(own_cert) {
                            certs.push(own_cert.clone());
                        }
                        nail_smime::mime::encrypt_entity(
                            &entity,
                            &certs,
                            nail_smime::ContentCipher::Aes256Gcm,
                        )?
                    }
                    None => entity.clone(),
                };
                out.push(OutgoingMessage {
                    envelope: Envelope {
                        from: message.envelope.from.clone(),
                        to: envelope_to,
                    },
                    raw: with_outer_headers(&raw, &protected),
                    message_id: message.message_id.clone(),
                });
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headers_split_and_join() {
        let raw = b"From: a@x\r\nSubject: long\r\n  folded\r\nContent-Type: text/plain\r\nMIME-Version: 1.0\r\n\r\nbody\r\n";
        let (fields, body) = split_header(raw);
        assert_eq!(
            fields.iter().map(|(n, _)| n.as_str()).collect::<Vec<_>>(),
            ["From", "Subject", "Content-Type", "MIME-Version"]
        );
        assert_eq!(fields[1].1, b"Subject: long\r\n  folded\r\n");
        assert_eq!(body, b"body\r\n");
        let entity = content_entity(raw);
        assert_eq!(entity, b"Content-Type: text/plain\r\n\r\nbody\r\n");
        let joined = with_outer_headers(raw, b"Content-Type: text/html\r\n\r\n<p>x</p>");
        assert_eq!(
            String::from_utf8(joined).unwrap(),
            "From: a@x\r\nSubject: long\r\n  folded\r\nMIME-Version: 1.0\r\nContent-Type: text/html\r\n\r\n<p>x</p>"
        );
        // LF-only input and a message without a Content-Type.
        let lf = b"From: a@x\nSubject: s\n\nhello\n";
        assert_eq!(
            content_entity(lf),
            b"Content-Type: text/plain; charset=us-ascii\r\n\r\nhello\n"
        );
    }

    #[test]
    fn encrypted_messages_are_recognized() {
        assert!(is_encrypted_raw(
            b"Content-Type: multipart/encrypted; protocol=\"application/pgp-encrypted\"; boundary=b\r\n\r\n"
        ));
        assert!(is_encrypted_raw(
            b"Content-Type: application/pkcs7-mime; smime-type=enveloped-data; name=smime.p7m\r\n\r\nMIA="
        ));
        assert!(is_encrypted_raw(
            b"Content-Type: text/plain\r\n\r\n-----BEGIN PGP MESSAGE-----\r\n\r\nhQ==\r\n-----END PGP MESSAGE-----\r\n"
        ));
        assert!(!is_encrypted_raw(b"Content-Type: text/plain\r\n\r\nhello"));
        assert!(!is_encrypted_raw(
            b"Content-Type: multipart/signed; protocol=\"application/pgp-signature\"; boundary=b\r\n\r\n"
        ));
    }

    #[test]
    fn crypto_choice_falls_back_to_the_identity() {
        let mut identity = Identity {
            id: "a@x".into(),
            account_id: "a".into(),
            email: "a@x".into(),
            name: None,
            reply_to: None,
            bcc: Vec::new(),
            signature: None,
            composer_mode: None,
            posting: None,
            mail_style: None,
            crypto: None,
        };
        assert!(!effective_crypto(None, None, &identity).is_active());
        identity.crypto = Some(IdentityCrypto {
            sign: true,
            ..IdentityCrypto::default()
        });
        assert!(effective_crypto(None, None, &identity).sign);
        let draft = SendCrypto {
            encrypt: true,
            ..SendCrypto::none()
        };
        assert!(effective_crypto(None, Some(&draft), &identity).encrypt);
        assert!(!effective_crypto(Some(SendCrypto::none()), Some(&draft), &identity).is_active());
    }
}
