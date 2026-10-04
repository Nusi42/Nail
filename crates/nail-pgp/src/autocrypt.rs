//! Autocrypt Level 1 (<https://autocrypt.org/level1.html>).
//!
//! * [`AutocryptHeader`]: the `Autocrypt` / `Autocrypt-Gossip` header
//!   (§2.1), parsed strictly (unknown critical attributes invalidate it)
//!   and written with a minimal key.
//! * [`process_incoming`]: the peer state update (§2.3) and gossip
//!   (§2.7), stored with `Store::put_autocrypt_peer`; keys go into the
//!   keyring with the source `autocrypt` / `autocrypt-gossip`.
//! * [`recommend`]: the encryption recommendation (§2.4).
//! * Setup messages (§4.4): [`create_setup_message`] /
//!   [`import_setup_message`], with the 36-digit numeric passcode.

use std::collections::BTreeMap;
use std::io::BufReader;

use base64::Engine as _;
use nail_api::AccountId;
use nail_api::security::{
    AutocryptPeer, AutocryptPreference, AutocryptRecommendation, CryptoTechnology, KeySource,
    RecipientKeyStatus, TrustLevel,
};
use nail_store::AutocryptPeerRecord;
use pgp::composed::{ArmorOptions, Message, MessageBuilder, SignedPublicKey, SignedSecretKey};
use pgp::crypto::hash::HashAlgorithm;
use pgp::crypto::sym::SymmetricKeyAlgorithm;
use pgp::types::{Password, StringToKey};
use rand::{Rng, thread_rng};
use zeroize::Zeroizing;

use crate::error::{Error, Problem, Result};
use crate::key::{self, KeyInfo, ParsedKey};
use crate::keyring::{Keyring, UnlockedKey, effective_trust, rfc3339};
use crate::mime::{self, Entity};

/// A peer's Autocrypt key is "stale" when it is this much older than the
/// newest message from the peer (§2.4.1).
pub const STALE_AFTER_SECS: i64 = 35 * 86_400;

/// Subject of setup messages (§4.4.1; recognised by the
/// `Autocrypt-Setup-Message` header, the subject is the spec's).
pub const SETUP_SUBJECT: &str = "Autocrypt Setup Message";

/// A parsed `Autocrypt` or `Autocrypt-Gossip` header.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AutocryptHeader {
    /// `addr` (lowercase).
    pub addr: String,
    /// `prefer-encrypt` (absent = `nopreference`; gossip has none).
    pub prefer_encrypt: AutocryptPreference,
    /// `keydata`: the binary transferable public key.
    pub keydata: Vec<u8>,
}

impl AutocryptHeader {
    /// Parses a header value. Fails without `addr` or `keydata`, with an
    /// unknown attribute that does not start with `_`, or with keydata
    /// that is not base64.
    pub fn parse(value: &str) -> Result<Self> {
        let mut addr = None;
        let mut prefer = AutocryptPreference::NoPreference;
        let mut keydata = None;
        for attribute in value.split(';') {
            let attribute = attribute.trim();
            if attribute.is_empty() {
                continue;
            }
            let (name, val) =
                attribute
                    .split_once('=')
                    .ok_or_else(|| Problem::AutocryptAttribute {
                        attribute: attribute.to_owned(),
                    })?;
            let name = name.trim().to_ascii_lowercase();
            let val = val.trim();
            match name.as_str() {
                "addr" => addr = Some(val.to_lowercase()),
                "prefer-encrypt" => {
                    prefer = if val.eq_ignore_ascii_case("mutual") {
                        AutocryptPreference::Mutual
                    } else {
                        AutocryptPreference::NoPreference
                    };
                }
                "keydata" => {
                    let compact: String = val.chars().filter(|c| !c.is_whitespace()).collect();
                    keydata = Some(
                        base64::engine::general_purpose::STANDARD
                            .decode(compact)
                            .map_err(|e| Problem::AutocryptKeydata {
                                detail: e.to_string(),
                            })?,
                    );
                }
                other if other.starts_with('_') => {}
                other => {
                    return Err(Problem::AutocryptCritical {
                        attribute: other.to_owned(),
                    }
                    .into());
                }
            }
        }
        let addr = addr
            .filter(|a| key::email_of_user_id(a).is_some())
            .ok_or(Problem::AutocryptMissing { attribute: "addr" })?;
        let keydata = keydata
            .filter(|k| !k.is_empty())
            .ok_or(Problem::AutocryptMissing {
                attribute: "keydata",
            })?;
        Ok(Self {
            addr,
            prefer_encrypt: prefer,
            keydata,
        })
    }

    /// The header for an own key: the minimal key for `addr` (§2.1.1).
    pub fn for_key(
        key: &SignedPublicKey,
        addr: &str,
        prefer_encrypt: AutocryptPreference,
        now: i64,
    ) -> Result<Self> {
        let minimal = key::minimal_for(key, addr, now)?;
        Ok(Self {
            addr: addr.trim().to_lowercase(),
            prefer_encrypt,
            keydata: key::to_binary(&minimal)?,
        })
    }

    /// The header value, keydata folded into 76-character lines
    /// (`with_header` / `Autocrypt-Gossip` ready). `gossip` omits
    /// `prefer-encrypt` (§2.7).
    pub fn to_value(&self, gossip: bool) -> String {
        let mut out = format!("addr={};", self.addr);
        if !gossip && self.prefer_encrypt == AutocryptPreference::Mutual {
            out.push_str(" prefer-encrypt=mutual;");
        }
        out.push_str(" keydata=");
        let encoded = base64::engine::general_purpose::STANDARD.encode(&self.keydata);
        for chunk in encoded.as_bytes().chunks(76) {
            out.push_str("\r\n ");
            out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        }
        out
    }

    /// The key it carries, restricted to `addr` ([`key::bind_to_address`]):
    /// a header vouches only for the binding of its key to its own
    /// address, so a key without a valid user id for `addr` is refused and
    /// its other user ids are dropped.
    pub fn bound_key(&self) -> Result<SignedPublicKey> {
        key::bind_to_address(&self.key()?, &self.addr)
    }

    /// The key it carries (as sent; see [`Self::bound_key`]).
    pub fn key(&self) -> Result<SignedPublicKey> {
        let (keys, issues) = key::parse_keys(&self.keydata);
        match keys.into_iter().next() {
            Some(ParsedKey::Public(k)) => Ok(*k),
            Some(ParsedKey::Secret(_)) => Err(Problem::AutocryptSecretKey.into()),
            // rPGP's reason, or Nail's own text when there is none.
            None => Err(issues.first().map_or(Problem::AutocryptNoKey.into(), |i| {
                Error::Crypto(i.1.clone())
            })),
        }
    }
}

/// The Autocrypt-relevant parts of a received message.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IncomingMessage {
    /// `From` addresses (lowercase).
    pub from: Vec<String>,
    /// `Date` (Unix seconds).
    pub date: Option<i64>,
    /// `To` and `Cc` addresses (lowercase).
    pub recipients: Vec<String>,
    /// Raw `Autocrypt` header values.
    pub autocrypt: Vec<String>,
    /// `multipart/report` (bounces etc. are ignored, §2.3).
    pub is_report: bool,
}

impl IncomingMessage {
    /// Reads the outer header of a raw message.
    pub fn from_raw(raw: &[u8]) -> Self {
        let entity = Entity::parse(raw);
        let list = |name: &str| {
            entity
                .headers
                .iter()
                .filter(|h| h.is(name))
                .flat_map(|h| mime::addresses(&h.value(), name))
                .collect::<Vec<_>>()
        };
        let mut recipients = list("To");
        recipients.extend(list("Cc"));
        Self {
            from: list("From"),
            date: entity.header_value("Date").and_then(|d| mime::date(&d)),
            recipients,
            autocrypt: entity
                .headers
                .iter()
                .filter(|h| h.is("Autocrypt"))
                .map(mime::HeaderField::value)
                .collect(),
            is_report: entity.content_type().is("multipart/report"),
        }
    }
}

/// What [`process_incoming`] changed.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AutocryptUpdate {
    /// The sender's peer state changed.
    pub peer_updated: bool,
    /// Keys stored or updated (sender and gossip).
    pub keys: Vec<nail_api::KeyId>,
}

fn new_peer(account: &AccountId, email: &str) -> AutocryptPeerRecord {
    AutocryptPeerRecord {
        account_id: account.clone(),
        email: email.to_owned(),
        last_seen: 0,
        autocrypt_timestamp: None,
        key_fingerprint: None,
        prefer_encrypt: AutocryptPreference::NoPreference,
        gossip_timestamp: None,
        gossip_fingerprint: None,
    }
}

/// Applies a received message to the Autocrypt state of `account`
/// (§2.3). `gossip` are the `Autocrypt-Gossip` values from the message's
/// decrypted part (`ReadOutcome::gossip`); pass none for unencrypted mail.
pub fn process_incoming(
    keyring: &Keyring<'_>,
    account: &AccountId,
    message: &IncomingMessage,
    gossip: &[String],
) -> Result<AutocryptUpdate> {
    let mut update = AutocryptUpdate::default();
    let [from] = message.from.as_slice() else {
        return Ok(update);
    };
    if message.is_report {
        return Ok(update);
    }
    let now = keyring.now();
    let effective = message.date.map_or(now, |d| d.min(now));
    let store = keyring.store();

    // The sender's header: exactly one valid header for the From address.
    let valid: Vec<(AutocryptHeader, SignedPublicKey)> = message
        .autocrypt
        .iter()
        .filter_map(|v| AutocryptHeader::parse(v).ok())
        .filter(|h| &h.addr == from)
        .filter_map(|h| h.bound_key().ok().map(|k| (h, k)))
        .collect();
    let header = match valid.as_slice() {
        [one] => Some(one.clone()),
        _ => None,
    };
    let mut peer = store
        .autocrypt_peer(account, from)?
        .unwrap_or_else(|| new_peer(account, from));
    let outdated = peer.autocrypt_timestamp.is_some_and(|t| effective < t);
    if !outdated {
        let mut changed = false;
        if effective > peer.last_seen {
            peer.last_seen = effective;
            changed = true;
        }
        if let Some((header, key)) = header {
            let (stored, _) = keyring.store_public(&key, KeySource::Autocrypt)?;
            peer.autocrypt_timestamp = Some(effective);
            peer.key_fingerprint = Some(stored.data.fingerprint.clone());
            peer.prefer_encrypt = header.prefer_encrypt;
            update.keys.push(stored.id);
            changed = true;
        }
        if changed {
            store.put_autocrypt_peer(&peer)?;
            update.peer_updated = true;
        }
    }

    // Gossip: only about the message's other recipients (§2.7).
    for value in gossip {
        let Ok(header) = AutocryptHeader::parse(value) else {
            continue;
        };
        if &header.addr == from || !message.recipients.contains(&header.addr) {
            continue;
        }
        let Ok(key) = header.bound_key() else {
            continue;
        };
        let mut peer = store
            .autocrypt_peer(account, &header.addr)?
            .unwrap_or_else(|| new_peer(account, &header.addr));
        if peer.gossip_timestamp.is_some_and(|t| effective <= t) {
            continue;
        }
        let (stored, _) = keyring.store_public(&key, KeySource::AutocryptGossip)?;
        peer.gossip_timestamp = Some(effective);
        peer.gossip_fingerprint = Some(stored.data.fingerprint.clone());
        store.put_autocrypt_peer(&peer)?;
        update.keys.push(stored.id);
    }
    Ok(update)
}

/// Whether the key with this fingerprint can encrypt now and is not
/// rejected by the user.
fn usable(keyring: &Keyring<'_>, fingerprint: Option<&str>) -> Result<bool> {
    let Some(fingerprint) = fingerprint else {
        return Ok(false);
    };
    let Some(stored) = keyring
        .store()
        .find_key(CryptoTechnology::OpenPgp, fingerprint)?
    else {
        return Ok(false);
    };
    if effective_trust(&stored) == TrustLevel::Rejected {
        return Ok(false);
    }
    let public = keyring.public_key(&stored)?;
    Ok(
        KeyInfo::analyze(&public)
            .is_ok_and(|info| info.encryption_component(keyring.now()).is_ok()),
    )
}

/// The preliminary recommendation for one recipient (§2.4.1).
pub fn preliminary(
    keyring: &Keyring<'_>,
    peer: Option<&AutocryptPeerRecord>,
    own_prefer: AutocryptPreference,
) -> Result<AutocryptRecommendation> {
    let Some(peer) = peer else {
        return Ok(AutocryptRecommendation::Disable);
    };
    let autocrypt_key = usable(keyring, peer.key_fingerprint.as_deref())?;
    let gossip_key = usable(keyring, peer.gossip_fingerprint.as_deref())?;
    if !autocrypt_key && !gossip_key {
        return Ok(AutocryptRecommendation::Disable);
    }
    if !autocrypt_key {
        return Ok(AutocryptRecommendation::Discourage);
    }
    let stale = peer
        .autocrypt_timestamp
        .is_some_and(|t| t < peer.last_seen - STALE_AFTER_SECS);
    if stale {
        return Ok(AutocryptRecommendation::Discourage);
    }
    if own_prefer == AutocryptPreference::Mutual
        && peer.prefer_encrypt == AutocryptPreference::Mutual
    {
        return Ok(AutocryptRecommendation::Encrypt);
    }
    Ok(AutocryptRecommendation::Available)
}

/// The recommendation for a message to `recipients` (§2.4.2). The
/// sender's own address in `recipients` is ignored. `own_key_usable`:
/// the sender has an own key to encrypt with.
pub fn recommend(
    keyring: &Keyring<'_>,
    account: &AccountId,
    own_address: &str,
    own_prefer: AutocryptPreference,
    own_key_usable: bool,
    recipients: &[String],
    reply_to_encrypted: bool,
) -> Result<AutocryptRecommendation> {
    let own = own_address.trim().to_lowercase();
    let others: Vec<String> = recipients
        .iter()
        .map(|r| r.trim().to_lowercase())
        .filter(|r| *r != own)
        .collect();
    if !own_key_usable || others.is_empty() {
        return Ok(AutocryptRecommendation::Disable);
    }
    let mut all = Vec::new();
    for email in &others {
        let peer = keyring.store().autocrypt_peer(account, email)?;
        all.push(preliminary(keyring, peer.as_ref(), own_prefer)?);
    }
    if all.contains(&AutocryptRecommendation::Disable) {
        return Ok(AutocryptRecommendation::Disable);
    }
    if reply_to_encrypted || all.iter().all(|r| *r == AutocryptRecommendation::Encrypt) {
        return Ok(AutocryptRecommendation::Encrypt);
    }
    if all.contains(&AutocryptRecommendation::Discourage) {
        return Ok(AutocryptRecommendation::Discourage);
    }
    Ok(AutocryptRecommendation::Available)
}

/// The protocol view of a peer record.
pub fn to_api_peer(record: &AutocryptPeerRecord) -> AutocryptPeer {
    AutocryptPeer {
        email: record.email.clone(),
        last_seen: rfc3339(record.last_seen),
        autocrypt_at: record.autocrypt_timestamp.map(rfc3339),
        prefer_encrypt: record.prefer_encrypt,
        key_fingerprint: record.key_fingerprint.clone(),
        gossip_fingerprint: record.gossip_fingerprint.clone(),
    }
}

/// The OpenPGP half of a recipient's key status (`CheckRecipientKeys`):
/// the best key and the Autocrypt peer; S/MIME and lookup are filled in
/// by the caller.
pub fn recipient_status(
    keyring: &Keyring<'_>,
    account: &AccountId,
    email: &str,
) -> Result<RecipientKeyStatus> {
    let email = email.trim().to_lowercase();
    let openpgp = keyring.recipient_key(&email)?.map(|(r, _)| r);
    let autocrypt = keyring
        .store()
        .autocrypt_peer(account, &email)?
        .map(|p| to_api_peer(&p));
    Ok(RecipientKeyStatus {
        email,
        openpgp,
        smime: None,
        autocrypt,
        lookup: nail_api::security::KeyLookupState::default(),
    })
}

// ---------------------------------------------------------------------------
// Setup messages (§4.4)
// ---------------------------------------------------------------------------

/// A new setup code: 36 random digits in nine blocks of four
/// (`numeric9x4`).
pub fn new_passcode() -> String {
    let mut rng = thread_rng();
    (0..9)
        .map(|_| format!("{:04}", rng.gen_range(0..10_000)))
        .collect::<Vec<_>>()
        .join("-")
}

/// Normalizes a typed passcode (spaces, missing dashes) to the
/// `1234-…-1234` form; `None` when it is not 36 digits.
fn normalize_passcode(input: &str) -> Option<String> {
    let digits: String = input.chars().filter(char::is_ascii_digit).collect();
    if digits.len() != 36 || input.chars().any(char::is_alphabetic) {
        return None;
    }
    Some(
        digits
            .as_bytes()
            .chunks(4)
            .map(|c| std::str::from_utf8(c).unwrap_or_default())
            .collect::<Vec<_>>()
            .join("-"),
    )
}

/// Localized texts of a setup message (the core fills them from the
/// Fluent catalogs; Nail writes no literal user-visible text).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetupMessageText {
    /// The text/plain part.
    pub body: String,
    /// The paragraph above the key in the HTML attachment.
    pub html_intro: String,
}

fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// Builds a setup message (to the sender itself) carrying `key`'s secret
/// key, encrypted with `passcode`.
pub fn create_setup_message(
    key: &UnlockedKey,
    addr: &str,
    prefer_encrypt: AutocryptPreference,
    passcode: &str,
    text: &SetupMessageText,
) -> Result<Vec<u8>> {
    let passcode = normalize_passcode(passcode).ok_or(Problem::SetupCodeFormat)?;
    // Unprotected transferable secret key (the passcode protects it).
    let mut secret = key.secret_key()?.clone();
    let password = key.password();
    secret.primary_key.remove_password(&password)?;
    for sub in &mut secret.secret_subkeys {
        sub.key.remove_password(&password)?;
    }
    let mut armor_headers = BTreeMap::new();
    armor_headers.insert(
        "Autocrypt-Prefer-Encrypt".to_owned(),
        vec![match prefer_encrypt {
            AutocryptPreference::Mutual => "mutual".to_owned(),
            AutocryptPreference::NoPreference => "nopreference".to_owned(),
        }],
    );
    let tsk = Zeroizing::new(secret.to_armored_string(ArmorOptions {
        headers: Some(&armor_headers),
        include_checksum: true,
    })?);

    let mut rng = thread_rng();
    let mut builder = MessageBuilder::from_bytes("", tsk.as_bytes().to_vec())
        .seipd_v1(&mut rng, SymmetricKeyAlgorithm::AES128);
    builder.encrypt_with_password(
        StringToKey::new_iterated(&mut rng, HashAlgorithm::Sha256, 255),
        &Password::from(passcode.as_str()),
    )?;
    let mut headers = BTreeMap::new();
    headers.insert(
        "Passphrase-Format".to_owned(),
        vec!["numeric9x4".to_owned()],
    );
    headers.insert(
        "Passphrase-Begin".to_owned(),
        vec![passcode[..2].to_owned()],
    );
    let armored = builder.to_armored_string(
        &mut rng,
        ArmorOptions {
            headers: Some(&headers),
            include_checksum: true,
        },
    )?;

    let addr = addr.trim();
    let boundary = mime::new_boundary();
    let domain = addr.rsplit_once('@').map_or("localhost", |(_, d)| d);
    let id: u64 = rng.r#gen();
    let date = chrono::Utc::now().to_rfc2822();
    let html = format!(
        "<html><body><p>{}</p><pre>\r\n{}</pre></body></html>",
        html_escape(&text.html_intro),
        armored
    );
    let message = format!(
        "From: <{addr}>\r\nTo: <{addr}>\r\nSubject: {SETUP_SUBJECT}\r\nDate: {date}\r\n\
         Message-ID: <autocrypt-setup-{id:016x}@{domain}>\r\nAutocrypt-Setup-Message: v1\r\n\
         MIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=\"{boundary}\"\r\n\r\n\
         --{boundary}\r\nContent-Type: text/plain; charset=utf-8\r\n\
         Content-Transfer-Encoding: base64\r\n\r\n{body}\r\n\
         --{boundary}\r\nContent-Type: application/autocrypt-setup\r\n\
         Content-Disposition: attachment; filename=\"autocrypt-setup-message.html\"\r\n\r\n\
         {html}\r\n--{boundary}--\r\n",
        body = base64_lines(text.body.as_bytes()),
    );
    Ok(mime::to_crlf(message.as_bytes()))
}

fn base64_lines(data: &[u8]) -> String {
    let encoded = base64::engine::general_purpose::STANDARD.encode(data);
    encoded
        .as_bytes()
        .chunks(76)
        .map(|c| std::str::from_utf8(c).unwrap_or_default())
        .collect::<Vec<_>>()
        .join("\r\n")
}

/// Whether a raw message is an Autocrypt setup message.
pub fn is_setup_message(raw: &[u8]) -> bool {
    Entity::parse(raw)
        .header_value("Autocrypt-Setup-Message")
        .is_some_and(|v| v.trim().eq_ignore_ascii_case("v1"))
}

/// The armored block of a setup message's `application/autocrypt-setup`
/// part (or anywhere in the message as a fallback).
fn setup_payload(raw: &[u8]) -> Option<String> {
    let entity = Entity::parse(raw);
    let ct = entity.content_type();
    let mut candidates: Vec<Vec<u8>> = Vec::new();
    if let Some(parts) = ct
        .param("boundary")
        .and_then(|b| mime::split_multipart(entity.body, b))
    {
        for part in parts.parts {
            if Entity::parse(part)
                .content_type()
                .is("application/autocrypt-setup")
            {
                candidates.push(mime::decoded_bytes(part).unwrap_or_else(|| part.to_vec()));
            }
        }
    }
    candidates.push(raw.to_vec());
    for data in candidates {
        let text = String::from_utf8_lossy(&data);
        if let Some(start) = text.find("-----BEGIN PGP MESSAGE-----")
            && let Some(end) = text[start..].find("-----END PGP MESSAGE-----")
        {
            let block = &text[start..start + end + "-----END PGP MESSAGE-----".len()];
            return Some(block.replace("\r\n", "\n"));
        }
    }
    None
}

/// Decrypts a setup message with the passcode: the secret key and its
/// `Autocrypt-Prefer-Encrypt`. [`Error::BadPassphrase`] for a wrong (or
/// malformed) passcode.
pub fn import_setup_message(
    raw: &[u8],
    passcode: &str,
) -> Result<(SignedSecretKey, AutocryptPreference)> {
    if !is_setup_message(raw) {
        return Err(Problem::NotSetupMessage.into());
    }
    let passcode = normalize_passcode(passcode).ok_or(Error::BadPassphrase)?;
    let payload = setup_payload(raw).ok_or(Problem::SetupWithoutPayload)?;
    let (message, _) =
        Message::from_armor(BufReader::new(payload.as_bytes())).map_err(Error::crypto)?;
    let mut decrypted = message
        .decrypt_with_password(&Password::from(passcode.as_str()))
        .map_err(|_| Error::BadPassphrase)?;
    if decrypted.is_compressed() {
        decrypted = decrypted.decompress().map_err(|_| Error::BadPassphrase)?;
    }
    let plaintext = Zeroizing::new(decrypted.as_data_vec().map_err(|_| Error::BadPassphrase)?);
    let text = String::from_utf8_lossy(&plaintext);
    let prefer = text
        .lines()
        .take_while(|l| !l.trim().is_empty() || l.starts_with("-----BEGIN"))
        .find_map(|l| l.strip_prefix("Autocrypt-Prefer-Encrypt:"))
        .map_or(AutocryptPreference::NoPreference, |v| {
            if v.trim().eq_ignore_ascii_case("mutual") {
                AutocryptPreference::Mutual
            } else {
                AutocryptPreference::NoPreference
            }
        });
    let (keys, _) = key::parse_keys(&plaintext);
    match keys.into_iter().next() {
        Some(ParsedKey::Secret(secret)) => Ok((*secret, prefer)),
        _ => Err(Problem::SetupWithoutSecretKey.into()),
    }
}

/// Imports the key of a setup message into the keyring (source
/// `setup-message`, protected with a keychain secret).
pub fn import_setup(
    keyring: &Keyring<'_>,
    raw: &[u8],
    passcode: &str,
) -> Result<(nail_api::security::KeyImportResult, AutocryptPreference)> {
    let (secret, prefer) = import_setup_message(raw, passcode)?;
    let (stored, created) = keyring.store_secret(&secret, KeySource::SetupMessage)?;
    Ok((
        nail_api::security::KeyImportResult {
            keys: vec![crate::keyring::to_crypto_key(&stored, keyring.now())],
            created: u32::from(created),
            updated: u32::from(!created),
            issues: Vec::new(),
        },
        prefer,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_parsing_is_strict() {
        let h = AutocryptHeader::parse(
            "addr=Ada@Example.org; prefer-encrypt=mutual; _x=1; keydata=AAEC Aw==",
        )
        .unwrap();
        assert_eq!(h.addr, "ada@example.org");
        assert_eq!(h.prefer_encrypt, AutocryptPreference::Mutual);
        assert_eq!(h.keydata, vec![0, 1, 2, 3]);
        assert!(AutocryptHeader::parse("addr=a@b.example; critical=1; keydata=AA==").is_err());
        assert!(AutocryptHeader::parse("keydata=AA==").is_err());
        assert!(AutocryptHeader::parse("addr=a@b.example").is_err());
        assert!(AutocryptHeader::parse("addr=a@b.example; keydata=!!").is_err());
        assert!(AutocryptHeader::parse("addr=nobody; keydata=AA==").is_err());
        let h =
            AutocryptHeader::parse("addr=a@b.example; prefer-encrypt=other; keydata=AA==").unwrap();
        assert_eq!(h.prefer_encrypt, AutocryptPreference::NoPreference);
    }

    #[test]
    fn header_value_folds_and_roundtrips() {
        let h = AutocryptHeader {
            addr: "a@b.example".into(),
            prefer_encrypt: AutocryptPreference::Mutual,
            keydata: vec![7u8; 200],
        };
        let value = h.to_value(false);
        assert!(value.starts_with("addr=a@b.example; prefer-encrypt=mutual; keydata=\r\n "));
        assert!(value.split("\r\n").all(|l| l.len() <= 78));
        let unfolded = value.replace("\r\n", "");
        assert_eq!(AutocryptHeader::parse(&unfolded).unwrap(), h);
        assert!(!h.to_value(true).contains("prefer-encrypt"));
    }

    #[test]
    fn passcodes() {
        let code = new_passcode();
        assert_eq!(code.len(), 44);
        assert_eq!(code.split('-').count(), 9);
        assert!(
            code.split('-')
                .all(|b| b.len() == 4 && b.chars().all(|c| c.is_ascii_digit()))
        );
        assert_ne!(new_passcode(), code);
        let spaced = code.replace('-', " ");
        assert_eq!(normalize_passcode(&spaced).as_deref(), Some(code.as_str()));
        assert_eq!(normalize_passcode("1234"), None);
        assert_eq!(normalize_passcode(&format!("{code}x")), None);
    }

    #[test]
    fn html_is_escaped() {
        assert_eq!(html_escape("<a & \"b\">"), "&lt;a &amp; &quot;b&quot;&gt;");
    }
}
