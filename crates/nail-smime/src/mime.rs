//! The S/MIME 4.0 MIME layer (RFC 8551 section 3): recognizing S/MIME
//! entities, producing signed / encrypted entities, and reading a message
//! down to its innermost content with a [`SecurityStatus`].
//!
//! Producing: [`sign_entity`] and [`encrypt_entity`] take a canonical MIME
//! entity (its own `Content-*` headers, a blank line and the body; CRLF
//! line ends; 7-bit transfer encodings, which the composer guarantees) and
//! return the protected entity. The caller puts the message headers
//! (`From`, `To`, `Subject`, `Date`, `MIME-Version`, …) in front of it.
//!
//! Reading: [`read`] follows `multipart/signed`, opaque `signed-data` and
//! `enveloped-data` / `authEnveloped-data` layers in any order (sign then
//! encrypt, triple wrap) up to [`MAX_LAYERS`] deep.

use base64::Engine as _;
use base64::engine::{DecodePaddingMode, GeneralPurpose, GeneralPurposeConfig};
use nail_api::security::{
    CryptoTechnology, DecryptionState, EncryptionInfo, HeaderProtection, SecurityStatus,
    SignatureInfo, SignatureState, TrustLevel,
};
use rand::RngCore;
use rand::rngs::OsRng;

use crate::cert::{Cert, hex_upper};
use crate::chain::TrustAnchors;
use crate::enveloped::{self, ContentCipher, Decryption, DecryptionKey};
use crate::error::{Error, Result};
use crate::signed::{self, Signer, SignerOutcome, VerifyContext};

/// How many protection layers [`read`] unwraps.
pub const MAX_LAYERS: usize = 4;

/// Lenient base64 for reading (missing padding is common).
const READ_B64: GeneralPurpose = GeneralPurpose::new(
    &base64::alphabet::STANDARD,
    GeneralPurposeConfig::new()
        .with_decode_padding_mode(DecodePaddingMode::Indifferent)
        .with_decode_allow_trailing_bits(true),
);

/// What kind of S/MIME entity something is.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SmimeKind {
    /// `multipart/signed` with a PKCS #7 signature (detached).
    MultipartSigned {
        /// The multipart boundary.
        boundary: String,
    },
    /// `application/pkcs7-mime; smime-type=signed-data` (opaque).
    SignedData,
    /// `application/pkcs7-mime; smime-type=enveloped-data`.
    EnvelopedData,
    /// `application/pkcs7-mime; smime-type=authEnveloped-data`.
    AuthEnvelopedData,
    /// `application/pkcs7-mime; smime-type=certs-only`.
    CertsOnly,
    /// Not S/MIME.
    None,
}

/// How to sign.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SignMode {
    /// `multipart/signed` (clear-signed): readable without S/MIME support.
    #[default]
    Detached,
    /// `application/pkcs7-mime; smime-type=signed-data`: survives
    /// gateways that rewrite text, unreadable without S/MIME.
    Opaque,
}

/// A parsed `Content-Type`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ContentType {
    pub mime: String,
    pub params: Vec<(String, String)>,
}

impl ContentType {
    pub(crate) fn param(&self, name: &str) -> Option<&str> {
        self.params
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(name))
            .map(|(_, v)| v.as_str())
    }
}

/// Splits an entity into its header block and body (after the first empty
/// line). An entity without headers starts with the empty line.
pub(crate) fn split_entity(entity: &[u8]) -> (&[u8], &[u8]) {
    if entity.starts_with(b"\r\n") {
        return (&[], &entity[2..]);
    }
    if entity.starts_with(b"\n") {
        return (&[], &entity[1..]);
    }
    let crlf = find(entity, b"\r\n\r\n").map(|i| (i + 2, i + 4));
    let lf = find(entity, b"\n\n").map(|i| (i + 1, i + 2));
    let split = match (crlf, lf) {
        (Some(a), Some(b)) => Some(if a.0 <= b.0 { a } else { b }),
        (a, b) => a.or(b),
    };
    match split {
        Some((head_end, body_start)) => (&entity[..head_end], &entity[body_start..]),
        None => (entity, &[]),
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

/// Unfolded headers as (lowercase name, value).
pub(crate) fn headers(block: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(block);
    let mut out: Vec<(String, String)> = Vec::new();
    for line in text.split('\n') {
        let line = line.strip_suffix('\r').unwrap_or(line);
        if line.is_empty() {
            continue;
        }
        if line.starts_with([' ', '\t']) {
            if let Some((_, value)) = out.last_mut() {
                value.push(' ');
                value.push_str(line.trim());
            }
            continue;
        }
        if let Some((name, value)) = line.split_once(':') {
            out.push((name.trim().to_ascii_lowercase(), value.trim().to_owned()));
        }
    }
    out
}

fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
    headers
        .iter()
        .find(|(n, _)| n == name)
        .map(|(_, v)| v.as_str())
}

/// Parses a `Content-Type` value (RFC 2045; quoted strings, comments are
/// not expected in the S/MIME headers that matter).
pub(crate) fn parse_content_type(value: &str) -> ContentType {
    let mut parts = split_params(value).into_iter();
    let mime = parts.next().unwrap_or_default().trim().to_ascii_lowercase();
    let params = parts
        .filter_map(|p| {
            let (k, v) = p.split_once('=')?;
            let v = v.trim();
            let v = if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
                unescape(&v[1..v.len() - 1])
            } else {
                v.to_owned()
            };
            Some((k.trim().to_ascii_lowercase(), v))
        })
        .collect();
    ContentType { mime, params }
}

fn split_params(value: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    let mut escaped = false;
    for c in value.chars() {
        if escaped {
            current.push(c);
            escaped = false;
            continue;
        }
        match c {
            '\\' if quoted => {
                current.push(c);
                escaped = true;
            }
            '"' => {
                quoted = !quoted;
                current.push(c);
            }
            ';' if !quoted => out.push(std::mem::take(&mut current)),
            _ => current.push(c),
        }
    }
    out.push(current);
    out
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(next) = chars.next() {
                out.push(next);
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Recognizes an S/MIME entity by its headers.
pub fn classify(entity: &[u8]) -> SmimeKind {
    let (head, _) = split_entity(entity);
    let headers = headers(head);
    let Some(ct) = header(&headers, "content-type").map(parse_content_type) else {
        return SmimeKind::None;
    };
    match ct.mime.as_str() {
        "multipart/signed" => {
            let protocol = ct
                .param("protocol")
                .unwrap_or_default()
                .to_ascii_lowercase();
            match (protocol.as_str(), ct.param("boundary")) {
                ("application/pkcs7-signature" | "application/x-pkcs7-signature", Some(b)) => {
                    SmimeKind::MultipartSigned {
                        boundary: b.to_owned(),
                    }
                }
                _ => SmimeKind::None,
            }
        }
        "application/pkcs7-mime" | "application/x-pkcs7-mime" => {
            match ct
                .param("smime-type")
                .map(str::to_ascii_lowercase)
                .as_deref()
            {
                Some("signed-data") => SmimeKind::SignedData,
                Some("enveloped-data") => SmimeKind::EnvelopedData,
                Some("authenveloped-data") => SmimeKind::AuthEnvelopedData,
                Some("certs-only") => SmimeKind::CertsOnly,
                _ => sniff(entity),
            }
        }
        // Some gateways relabel the part; trust the file name.
        "application/octet-stream" => {
            let filename = header(&headers, "content-disposition")
                .map(parse_content_type)
                .and_then(|d| d.param("filename").map(str::to_owned));
            let name = ct.param("name").map(str::to_owned).or(filename);
            if name.is_some_and(|n| n.to_ascii_lowercase().ends_with(".p7m")) {
                sniff(entity)
            } else {
                SmimeKind::None
            }
        }
        _ => SmimeKind::None,
    }
}

/// Decides by the CMS content type when the MIME labels are missing.
fn sniff(entity: &[u8]) -> SmimeKind {
    let Ok(der) = decode_body(entity).and_then(|b| crate::ber::normalize(&b)) else {
        return SmimeKind::None;
    };
    let Ok(info) = <cms::content_info::ContentInfo as der::Decode>::from_der(&der) else {
        return SmimeKind::None;
    };
    match info.content_type {
        crate::oids::ID_SIGNED_DATA => {
            let has_signers = info
                .content
                .decode_as::<cms::signed_data::SignedData>()
                .is_ok_and(|s| !s.signer_infos.0.is_empty());
            if has_signers {
                SmimeKind::SignedData
            } else {
                SmimeKind::CertsOnly
            }
        }
        crate::oids::ID_ENVELOPED_DATA => SmimeKind::EnvelopedData,
        crate::oids::ID_AUTH_ENVELOPED_DATA => SmimeKind::AuthEnvelopedData,
        _ => SmimeKind::None,
    }
}

/// The decoded body of an entity (base64, or binary / 7bit / 8bit as is).
pub(crate) fn decode_body(entity: &[u8]) -> Result<Vec<u8>> {
    let (head, body) = split_entity(entity);
    let headers = headers(head);
    let cte = header(&headers, "content-transfer-encoding")
        .unwrap_or("7bit")
        .to_ascii_lowercase();
    match cte.as_str() {
        "base64" => {
            let compact: Vec<u8> = body
                .iter()
                .copied()
                .filter(|b| !b.is_ascii_whitespace())
                .collect();
            READ_B64
                .decode(compact)
                .map_err(|_| Error::malformed("bad base64 body"))
        }
        "7bit" | "8bit" | "binary" => Ok(body.to_vec()),
        other => Err(Error::Unsupported(format!("transfer encoding {other}"))),
    }
}

/// The parts of a multipart body, byte-exact: each part ends before the
/// line break that precedes the next delimiter (RFC 2046 section 5.1.1).
pub(crate) fn split_multipart<'a>(body: &'a [u8], boundary: &str) -> Result<Vec<&'a [u8]>> {
    let delimiter = format!("--{boundary}");
    let delimiter = delimiter.as_bytes();
    let mut parts = Vec::new();
    let mut part_start: Option<usize> = None;
    let mut line_start = 0usize;
    let mut closed = false;
    while line_start <= body.len() {
        let line_end = body[line_start..]
            .iter()
            .position(|b| *b == b'\n')
            .map_or(body.len(), |p| line_start + p);
        let line = &body[line_start..line_end];
        if line.starts_with(delimiter) {
            let rest = &line[delimiter.len()..];
            let is_close = rest.starts_with(b"--");
            let tail = if is_close { &rest[2..] } else { rest };
            if tail.iter().all(|b| matches!(b, b' ' | b'\t' | b'\r')) {
                if let Some(start) = part_start {
                    let mut end = line_start;
                    if end > start && body[end - 1] == b'\n' {
                        end -= 1;
                        if end > start && body[end - 1] == b'\r' {
                            end -= 1;
                        }
                    }
                    parts.push(&body[start..end.max(start)]);
                }
                if is_close {
                    closed = true;
                    break;
                }
                part_start = Some((line_end + 1).min(body.len()));
            }
        }
        line_start = line_end + 1;
    }
    if !closed && part_start.is_some() {
        // Tolerate a missing close delimiter: the last part runs to the end.
        if let Some(start) = part_start {
            parts.push(&body[start..]);
        }
    }
    if parts.is_empty() {
        return Err(Error::malformed("multipart body without parts"));
    }
    Ok(parts)
}

/// Converts bare LF line ends to CRLF (MIME canonical form).
pub fn canonicalize(entity: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(entity.len() + entity.len() / 32);
    let mut previous = 0u8;
    for &b in entity {
        if b == b'\n' && previous != b'\r' {
            out.push(b'\r');
        }
        out.push(b);
        previous = b;
    }
    out
}

fn base64_lines(data: &[u8]) -> String {
    let b64 = base64::engine::general_purpose::STANDARD.encode(data);
    let mut out = String::with_capacity(b64.len() + b64.len() / 38 + 2);
    for chunk in b64.as_bytes().chunks(76) {
        out.push_str(std::str::from_utf8(chunk).unwrap_or_default());
        out.push_str("\r\n");
    }
    out
}

fn pkcs7_mime_entity(smime_type: &str, der: &[u8]) -> Vec<u8> {
    format!(
        "Content-Type: application/pkcs7-mime; smime-type={smime_type}; name=\"smime.p7m\"\r\n\
         Content-Transfer-Encoding: base64\r\n\
         Content-Disposition: attachment; filename=\"smime.p7m\"\r\n\
         \r\n{}",
        base64_lines(der)
    )
    .into_bytes()
}

/// Signs a MIME entity; returns the signed entity (see the module docs).
pub fn sign_entity(
    entity: &[u8],
    signer: &Signer<'_>,
    mode: SignMode,
    signing_time: i64,
) -> Result<Vec<u8>> {
    let entity = canonicalize(entity);
    match mode {
        SignMode::Opaque => {
            let der = signed::sign(&entity, signer, false, signing_time)?;
            Ok(pkcs7_mime_entity("signed-data", &der))
        }
        SignMode::Detached => {
            let der = signed::sign(&entity, signer, true, signing_time)?;
            let micalg = signer.cert.public_key()?.preferred_hash().micalg();
            let mut random = [0u8; 12];
            OsRng.fill_bytes(&mut random);
            let boundary = format!("----=_NailSmime_{}", hex_upper(&random));
            let mut out = format!(
                "Content-Type: multipart/signed; protocol=\"application/pkcs7-signature\"; \
                 micalg={micalg}; boundary=\"{boundary}\"\r\n\r\n--{boundary}\r\n"
            )
            .into_bytes();
            out.extend_from_slice(&entity);
            out.extend_from_slice(
                format!(
                    "\r\n--{boundary}\r\n\
                     Content-Type: application/pkcs7-signature; name=\"smime.p7s\"\r\n\
                     Content-Transfer-Encoding: base64\r\n\
                     Content-Disposition: attachment; filename=\"smime.p7s\"\r\n\
                     \r\n{}--{boundary}--\r\n",
                    base64_lines(&der)
                )
                .as_bytes(),
            );
            Ok(out)
        }
    }
}

/// Encrypts a MIME entity to the certificates (add the sender's own to
/// keep a readable copy); returns the `application/pkcs7-mime` entity.
pub fn encrypt_entity(
    entity: &[u8],
    recipients: &[Cert],
    cipher: ContentCipher,
) -> Result<Vec<u8>> {
    let entity = canonicalize(entity);
    let der = enveloped::encrypt(&entity, recipients, cipher)?;
    Ok(pkcs7_mime_entity(cipher.smime_type(), &der))
}

/// What [`read`] needs.
#[derive(Clone, Copy)]
pub struct ReadContext<'a> {
    /// Trust anchors for certificate chains.
    pub anchors: &'a dyn TrustAnchors,
    /// Certificates beyond the message (the keyring).
    pub known_certs: &'a [Cert],
    /// Own certificates and their keys, for decryption.
    pub keys: &'a [DecryptionKey<'a>],
    /// The message's `From` address.
    pub from: Option<&'a str>,
    /// Now (Unix seconds).
    pub now: i64,
}

impl std::fmt::Debug for ReadContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ReadContext")
            .field("known_certs", &self.known_certs)
            .field("keys", &self.keys.len())
            .field("from", &self.from)
            .field("now", &self.now)
            .finish_non_exhaustive()
    }
}

/// The result of reading an S/MIME message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReadOutcome {
    /// For `MessageBody::security` (`key_id` and `trust` are filled in by
    /// the keyring).
    pub status: SecurityStatus,
    /// The innermost entity to display: the signed content, the decrypted
    /// content, or the still-encrypted entity when decryption failed.
    pub content: Vec<u8>,
    /// Signers with their certificates (same order as
    /// `status.signatures`).
    pub signers: Vec<SignerOutcome>,
    /// Every certificate the message carried (for remembering signers'
    /// certificates and intermediates).
    pub certificates: Vec<Cert>,
    /// Fingerprint of the own certificate that decrypted.
    pub decrypted_with: Option<String>,
    /// Fingerprint of the own certificate whose key is locked and would
    /// decrypt (`EncryptionInfo.state` is `locked`).
    pub locked: Option<String>,
}

fn error_signature() -> SignerOutcome {
    SignerOutcome {
        info: SignatureInfo {
            state: SignatureState::Error,
            signer: None,
            signer_matches_from: false,
            key_id: None,
            fingerprint: None,
            trust: TrustLevel::Unknown,
            signed_at: None,
            chain: None,
        },
        cert: None,
    }
}

/// Reads an entity (a whole message or a body part). `Ok(None)` when it
/// is not signed or encrypted with S/MIME.
pub fn read(entity: &[u8], ctx: &ReadContext<'_>) -> Result<Option<ReadOutcome>> {
    let verify_ctx = VerifyContext {
        anchors: ctx.anchors,
        known_certs: ctx.known_certs,
        from: ctx.from,
        now: ctx.now,
    };
    let mut outcome = ReadOutcome {
        status: SecurityStatus {
            technology: CryptoTechnology::Smime,
            signatures: Vec::new(),
            encryption: None,
            header_protection: HeaderProtection::None,
            partial: false,
            inline_pgp: false,
        },
        content: entity.to_vec(),
        signers: Vec::new(),
        certificates: Vec::new(),
        decrypted_with: None,
        locked: None,
    };
    let mut touched = false;
    for _ in 0..MAX_LAYERS {
        let current = std::mem::take(&mut outcome.content);
        match classify(&current) {
            SmimeKind::MultipartSigned { boundary } => {
                let (_, body) = split_entity(&current);
                let parts = split_multipart(body, &boundary)?;
                let (Some(signed_part), Some(signature_part)) = (parts.first(), parts.get(1))
                else {
                    return Err(Error::malformed("multipart/signed needs two parts"));
                };
                let signers = decode_body(signature_part)
                    .and_then(|der| verify_detached(&der, signed_part, &verify_ctx));
                match signers {
                    Ok(verified) => {
                        outcome.signers.extend(verified.signers);
                        outcome.certificates.extend(verified.certificates);
                    }
                    Err(_) => outcome.signers.push(error_signature()),
                }
                outcome.content = signed_part.to_vec();
            }
            SmimeKind::SignedData => {
                let verified =
                    decode_body(&current).and_then(|der| signed::verify(&der, None, &verify_ctx));
                if let Ok(verified) = verified {
                    outcome.signers.extend(verified.signers);
                    outcome.certificates.extend(verified.certificates);
                    outcome.content = verified.content;
                } else {
                    outcome.signers.push(error_signature());
                    outcome.content = current;
                    touched = true;
                    break;
                }
            }
            SmimeKind::EnvelopedData | SmimeKind::AuthEnvelopedData => {
                touched = true;
                let Ok(der) = decode_body(&current) else {
                    outcome.status.encryption = Some(encryption(DecryptionState::Failed, None));
                    outcome.content = current;
                    break;
                };
                match enveloped::decrypt(&der, ctx.keys) {
                    Ok((
                        envelope,
                        Decryption::Decrypted {
                            plaintext,
                            fingerprint,
                        },
                    )) => {
                        outcome.status.encryption = Some(EncryptionInfo {
                            state: DecryptionState::Decrypted,
                            key_id: None,
                            recipients: envelope.recipients,
                            algorithm: Some(envelope.algorithm),
                        });
                        outcome.decrypted_with = Some(fingerprint);
                        outcome.content = plaintext.to_vec();
                        continue;
                    }
                    Ok((envelope, Decryption::NoKey)) => {
                        outcome.status.encryption = Some(EncryptionInfo {
                            recipients: envelope.recipients,
                            ..encryption(DecryptionState::NoKey, Some(envelope.algorithm))
                        });
                    }
                    Ok((envelope, Decryption::Locked { fingerprint })) => {
                        outcome.status.encryption = Some(EncryptionInfo {
                            recipients: envelope.recipients,
                            ..encryption(DecryptionState::Locked, Some(envelope.algorithm))
                        });
                        outcome.locked = Some(fingerprint);
                    }
                    Err(err) => {
                        let state = match err {
                            Error::Unsupported(_) => DecryptionState::Unsupported,
                            _ => DecryptionState::Failed,
                        };
                        let envelope = enveloped::inspect(&der).ok();
                        outcome.status.encryption = Some(EncryptionInfo {
                            recipients: envelope
                                .as_ref()
                                .map(|e| e.recipients.clone())
                                .unwrap_or_default(),
                            ..encryption(state, envelope.map(|e| e.algorithm))
                        });
                    }
                }
                outcome.content = current;
                break;
            }
            SmimeKind::CertsOnly | SmimeKind::None => {
                outcome.content = current;
                break;
            }
        }
        touched = true;
    }
    if !touched {
        return Ok(None);
    }
    outcome.status.signatures = outcome.signers.iter().map(|s| s.info.clone()).collect();
    Ok(Some(outcome))
}

fn encryption(state: DecryptionState, algorithm: Option<String>) -> EncryptionInfo {
    EncryptionInfo {
        state,
        key_id: None,
        recipients: Vec::new(),
        algorithm,
    }
}

/// Verifies a detached signature over the signed part as sent, and again
/// over its canonical (CRLF) form when a gateway turned CRLF into LF.
fn verify_detached(
    der: &[u8],
    signed_part: &[u8],
    ctx: &VerifyContext<'_>,
) -> Result<signed::Verified> {
    let verified = signed::verify(der, Some(signed_part), ctx)?;
    let invalid = verified
        .signers
        .iter()
        .any(|s| s.info.state == SignatureState::Invalid);
    if invalid {
        let canonical = canonicalize(signed_part);
        if canonical != signed_part {
            let again = signed::verify(der, Some(&canonical), ctx)?;
            if again
                .signers
                .iter()
                .all(|s| s.info.state != SignatureState::Invalid)
            {
                return Ok(again);
            }
        }
    }
    Ok(verified)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain::FixedTrustAnchors;
    use crate::keys::SoftwareKey;
    use crate::testutil::{AT, fixture, fixture_cert, fixture_key, test_anchors};
    use nail_api::security::ChainState;
    use pretty_assertions::assert_eq;

    struct World {
        anchors: FixedTrustAnchors,
        known: Vec<Cert>,
        alice: (Cert, SoftwareKey),
        bob: (Cert, SoftwareKey),
    }

    fn world() -> World {
        World {
            anchors: test_anchors(),
            known: vec![fixture_cert("intermediate-ca.crt")],
            alice: (fixture_cert("alice.crt"), fixture_key("alice.key")),
            bob: (fixture_cert("bob.crt"), fixture_key("bob.key")),
        }
    }

    fn read_as(world: &World, who: &str, entity: &[u8], from: &str) -> Option<ReadOutcome> {
        let (cert, key) = if who == "alice" {
            &world.alice
        } else {
            &world.bob
        };
        let keys = [DecryptionKey {
            cert,
            key: Some(key),
        }];
        let ctx = ReadContext {
            anchors: &world.anchors,
            known_certs: &world.known,
            keys: &keys,
            from: Some(from),
            now: AT,
        };
        read(entity, &ctx).unwrap()
    }

    fn signature(outcome: &ReadOutcome) -> &SignatureInfo {
        let [signature] = outcome.status.signatures.as_slice() else {
            panic!("one signature expected: {:?}", outcome.status)
        };
        signature
    }

    fn chain_state(signature: &SignatureInfo) -> ChainState {
        signature.chain.as_ref().expect("chain").state
    }

    #[test]
    fn openssl_signatures_verify() {
        let w = world();
        let content = fixture("content.eml");
        for (file, from) in [
            ("alice-signed-detached.eml", "alice@example.org"),
            ("alice-signed-opaque.eml", "alice@example.org"),
            ("alice-signed-pss.eml", "alice@example.org"),
            ("alice-signed-noattr.eml", "alice@example.org"),
            ("bob-signed-detached.eml", "bob@example.net"),
        ] {
            let outcome = read_as(&w, "bob", &fixture(file), from).expect(file);
            let sig = signature(&outcome);
            assert_eq!(sig.state, SignatureState::Valid, "{file}");
            assert!(sig.signer_matches_from, "{file}");
            assert_eq!(chain_state(sig), ChainState::Valid, "{file}");
            assert_eq!(outcome.content, content, "{file}");
            assert!(outcome.status.encryption.is_none());
            assert_eq!(outcome.status.technology, CryptoTechnology::Smime);
            assert!(
                !outcome.certificates.is_empty(),
                "{:?}",
                outcome.certificates
            );
            assert_eq!(sig.signed_at.is_none(), file.contains("noattr"), "{file}");
        }
    }

    #[test]
    fn openssl_signature_problems_are_reported() {
        let w = world();
        let expired = read_as(
            &w,
            "bob",
            &fixture("carol-signed-expired.eml"),
            "carol@example.org",
        )
        .unwrap();
        assert_eq!(signature(&expired).state, SignatureState::KeyExpired);
        assert_eq!(chain_state(signature(&expired)), ChainState::Expired);

        let usage = read_as(
            &w,
            "bob",
            &fixture("dave-signed-wrong-usage.eml"),
            "dave@example.org",
        )
        .unwrap();
        assert_eq!(signature(&usage).state, SignatureState::Valid);
        assert_eq!(chain_state(signature(&usage)), ChainState::WrongUsage);

        // Mallory signs with a certificate from an unknown CA and claims
        // to be alice in From.
        let untrusted = read_as(
            &w,
            "bob",
            &fixture("mallory-signed-untrusted.eml"),
            "alice@example.org",
        )
        .unwrap();
        let sig = signature(&untrusted);
        assert_eq!(sig.state, SignatureState::Valid);
        assert!(!sig.signer_matches_from);
        assert_eq!(sig.signer.as_deref(), Some("mallory@example.org"));
        assert_eq!(chain_state(sig), ChainState::Incomplete);
        let mut with_root = world();
        with_root.known.push(fixture_cert("evil-root-ca.crt"));
        let untrusted = read_as(
            &with_root,
            "bob",
            &fixture("mallory-signed-untrusted.eml"),
            "alice@example.org",
        )
        .unwrap();
        assert_eq!(
            chain_state(signature(&untrusted)),
            ChainState::UntrustedRoot
        );
    }

    #[test]
    fn tampered_signed_text_is_invalid() {
        let w = world();
        let original = fixture("alice-signed-detached.eml");
        let pos = find(&original, b"Hello Bob").unwrap();
        let mut tampered = original.clone();
        tampered[pos] = b'J';
        let outcome = read_as(&w, "bob", &tampered, "alice@example.org").unwrap();
        assert_eq!(signature(&outcome).state, SignatureState::Invalid);

        // Opaque: change one base64 character of the payload.
        let original = fixture("alice-signed-opaque.eml");
        let (_, body) = split_entity(&original);
        let offset = original.len() - body.len() + 200;
        let mut tampered = original.clone();
        tampered[offset] = if tampered[offset] == b'A' { b'B' } else { b'A' };
        let outcome = read_as(&w, "bob", &tampered, "alice@example.org").unwrap();
        assert_ne!(signature(&outcome).state, SignatureState::Valid);
    }

    #[test]
    fn a_gateway_that_strips_cr_does_not_break_the_signature() {
        let w = world();
        let lf: Vec<u8> = fixture("alice-signed-detached.eml")
            .into_iter()
            .filter(|b| *b != b'\r')
            .collect();
        let outcome = read_as(&w, "bob", &lf, "alice@example.org").unwrap();
        assert_eq!(signature(&outcome).state, SignatureState::Valid);
    }

    #[test]
    fn openssl_encryption_decrypts() {
        let w = world();
        let content = fixture("content.eml");
        for (file, who, algorithm) in [
            ("alice-enc-cbc.eml", "alice", "AES-256-CBC"),
            ("alice-enc-gcm.eml", "alice", "AES-256-GCM"),
            ("alice-enc-oaep.eml", "alice", "AES-256-GCM"),
            ("bob-enc-gcm.eml", "bob", "AES-256-GCM"),
            ("both-enc-gcm128.eml", "alice", "AES-128-GCM"),
            ("both-enc-gcm128.eml", "bob", "AES-128-GCM"),
        ] {
            let outcome = read_as(&w, who, &fixture(file), "alice@example.org").expect(file);
            let enc = outcome.status.encryption.as_ref().expect("encryption");
            assert_eq!(enc.state, DecryptionState::Decrypted, "{file} as {who}");
            assert_eq!(enc.algorithm.as_deref(), Some(algorithm), "{file}");
            assert!(!enc.recipients.is_empty(), "{:?}", enc.recipients);
            assert_eq!(outcome.content, content, "{file} as {who}");
            assert!(
                outcome.status.signatures.is_empty(),
                "{:?}",
                outcome.status.signatures
            );
            let expected = if who == "alice" { &w.alice.0 } else { &w.bob.0 };
            assert_eq!(
                outcome.decrypted_with.as_deref(),
                Some(expected.fingerprint())
            );
        }
    }

    #[test]
    fn wrong_recipient_and_damage() {
        let w = world();
        let outcome = read_as(
            &w,
            "bob",
            &fixture("alice-enc-gcm.eml"),
            "alice@example.org",
        )
        .unwrap();
        let enc = outcome.status.encryption.as_ref().unwrap();
        assert_eq!(enc.state, DecryptionState::NoKey);
        assert_eq!(enc.recipients.len(), 1);
        assert!(enc.recipients[0].starts_with("CN=Nail Test Email CA,O=Nail Test/"));
        // The still-encrypted entity is what remains.
        assert_eq!(classify(&outcome.content), SmimeKind::AuthEnvelopedData);

        let original = fixture("alice-enc-gcm.eml");
        let mut tampered = original.clone();
        let offset = tampered.len() - 40;
        tampered[offset] = if tampered[offset] == b'A' { b'B' } else { b'A' };
        let outcome = read_as(&w, "alice", &tampered, "alice@example.org").unwrap();
        assert_eq!(
            outcome.status.encryption.as_ref().unwrap().state,
            DecryptionState::Failed
        );
    }

    #[test]
    fn signed_then_encrypted_by_openssl() {
        let w = world();
        let outcome = read_as(
            &w,
            "bob",
            &fixture("alice-signed-enc-bob.eml"),
            "alice@example.org",
        )
        .unwrap();
        assert_eq!(
            outcome.status.encryption.as_ref().unwrap().state,
            DecryptionState::Decrypted
        );
        assert_eq!(signature(&outcome).state, SignatureState::Valid);
        assert_eq!(outcome.content, fixture("content.eml"));
        assert!(outcome.status.all_signatures_valid());
    }

    #[test]
    fn locked_key_is_reported() {
        let w = world();
        let keys = [DecryptionKey {
            cert: &w.alice.0,
            key: None,
        }];
        let ctx = ReadContext {
            anchors: &w.anchors,
            known_certs: &w.known,
            keys: &keys,
            from: None,
            now: AT,
        };
        let outcome = read(&fixture("alice-enc-cbc.eml"), &ctx).unwrap().unwrap();
        assert_eq!(
            outcome.status.encryption.as_ref().unwrap().state,
            DecryptionState::Locked
        );
        assert_eq!(outcome.locked.as_deref(), Some(w.alice.0.fingerprint()));
    }

    const ENTITY: &[u8] = b"Content-Type: text/plain; charset=utf-8\r\n\r\nHi Bob\r\n";

    #[test]
    fn nail_sign_and_encrypt_round_trip() {
        let w = world();
        let chain = [fixture_cert("intermediate-ca.crt")];
        let signer = Signer {
            key: &w.alice.1,
            cert: &w.alice.0,
            chain: &chain,
        };
        for mode in [SignMode::Detached, SignMode::Opaque] {
            let signed_mail = sign_entity(ENTITY, &signer, mode, AT).unwrap();
            let expected_kind = match mode {
                SignMode::Detached => "multipart/signed",
                SignMode::Opaque => "application/pkcs7-mime",
            };
            assert!(
                String::from_utf8_lossy(&signed_mail)
                    .starts_with(&format!("Content-Type: {expected_kind}"))
            );
            let outcome = read_as(&w, "bob", &signed_mail, "alice@example.org").unwrap();
            assert_eq!(signature(&outcome).state, SignatureState::Valid, "{mode:?}");
            assert_eq!(chain_state(signature(&outcome)), ChainState::Valid);
            assert_eq!(outcome.content, ENTITY);

            // Sign, then encrypt to bob and to alice herself.
            let encrypted = encrypt_entity(
                &signed_mail,
                &[w.bob.0.clone(), w.alice.0.clone()],
                ContentCipher::Aes256Gcm,
            )
            .unwrap();
            assert_eq!(classify(&encrypted), SmimeKind::AuthEnvelopedData);
            for reader in ["alice", "bob"] {
                let outcome = read_as(&w, reader, &encrypted, "alice@example.org").unwrap();
                assert_eq!(
                    outcome.status.encryption.as_ref().unwrap().state,
                    DecryptionState::Decrypted
                );
                assert_eq!(signature(&outcome).state, SignatureState::Valid);
                assert_eq!(outcome.content, ENTITY);
            }
        }
        let cbc = encrypt_entity(
            ENTITY,
            std::slice::from_ref(&w.bob.0),
            ContentCipher::Aes128Cbc,
        )
        .unwrap();
        assert_eq!(classify(&cbc), SmimeKind::EnvelopedData);
        let outcome = read_as(&w, "bob", &cbc, "x@example.org").unwrap();
        assert_eq!(outcome.content, ENTITY);
    }

    #[test]
    fn lf_input_is_canonicalized_before_signing() {
        let w = world();
        let signer = Signer {
            key: &w.alice.1,
            cert: &w.alice.0,
            chain: &[],
        };
        let lf = b"Content-Type: text/plain\n\nline one\nline two\n";
        let signed_mail = sign_entity(lf, &signer, SignMode::Detached, AT).unwrap();
        assert!(
            !signed_mail
                .windows(2)
                .any(|p| p[1] == b'\n' && p[0] != b'\r')
        );
        let outcome = read_as(&w, "bob", &signed_mail, "alice@example.org").unwrap();
        assert_eq!(signature(&outcome).state, SignatureState::Valid);
        assert_eq!(outcome.content, canonicalize(lf));
    }

    /// Writes Nail's own output for the manual OpenSSL interoperability
    /// check when `NAIL_SMIME_INTEROP_DIR` is set (see the corpus README);
    /// otherwise it only checks that producing it works.
    #[test]
    fn interop_output_for_openssl() {
        let w = world();
        let chain = [fixture_cert("intermediate-ca.crt")];
        let alice = Signer {
            key: &w.alice.1,
            cert: &w.alice.0,
            chain: &chain,
        };
        let bob = Signer {
            key: &w.bob.1,
            cert: &w.bob.0,
            chain: &chain,
        };
        let mut out: Vec<(&str, Vec<u8>)> = vec![
            (
                "nail-signed-detached.eml",
                sign_entity(ENTITY, &alice, SignMode::Detached, AT).unwrap(),
            ),
            (
                "nail-signed-opaque.eml",
                sign_entity(ENTITY, &alice, SignMode::Opaque, AT).unwrap(),
            ),
            (
                "nail-signed-ecdsa.eml",
                sign_entity(ENTITY, &bob, SignMode::Detached, AT).unwrap(),
            ),
        ];
        for (name, cipher, to) in [
            (
                "nail-enc-gcm-alice.eml",
                ContentCipher::Aes256Gcm,
                &w.alice.0,
            ),
            (
                "nail-enc-cbc-alice.eml",
                ContentCipher::Aes256Cbc,
                &w.alice.0,
            ),
            ("nail-enc-gcm-bob.eml", ContentCipher::Aes256Gcm, &w.bob.0),
            (
                "nail-enc-gcm128-bob.eml",
                ContentCipher::Aes128Gcm,
                &w.bob.0,
            ),
        ] {
            out.push((
                name,
                encrypt_entity(ENTITY, std::slice::from_ref(to), cipher).unwrap(),
            ));
        }
        let Some(dir) = std::env::var_os("NAIL_SMIME_INTEROP_DIR") else {
            return;
        };
        let dir = std::path::PathBuf::from(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for (name, entity) in out {
            let mut message = b"From: alice@example.org\r\nTo: bob@example.net\r\nSubject: Nail interop\r\nMIME-Version: 1.0\r\n".to_vec();
            message.extend_from_slice(&entity);
            std::fs::write(dir.join(name), message).unwrap();
        }
    }

    #[test]
    fn not_smime_is_none() {
        let w = world();
        let plain = b"From: a@example.org\r\nContent-Type: text/plain\r\n\r\nhello\r\n";
        assert!(read_as(&w, "bob", plain, "a@example.org").is_none());
        assert_eq!(classify(plain), SmimeKind::None);
        assert_eq!(classify(b"no headers at all"), SmimeKind::None);
        let pgp = b"Content-Type: multipart/signed; protocol=\"application/pgp-signature\"; boundary=x\r\n\r\n";
        assert_eq!(classify(pgp), SmimeKind::None);
    }

    #[test]
    fn classify_by_labels_and_by_content() {
        let detached = fixture("alice-signed-detached.eml");
        assert!(matches!(
            classify(&detached),
            SmimeKind::MultipartSigned { boundary } if boundary.starts_with("----")
        ));
        assert_eq!(
            classify(&fixture("alice-signed-opaque.eml")),
            SmimeKind::SignedData
        );
        assert_eq!(
            classify(&fixture("alice-enc-cbc.eml")),
            SmimeKind::EnvelopedData
        );
        assert_eq!(
            classify(&fixture("alice-enc-gcm.eml")),
            SmimeKind::AuthEnvelopedData
        );
        // Relabelled as octet-stream, and without smime-type.
        let relabel = |from: &str, to: &str| {
            String::from_utf8(fixture("alice-enc-gcm.eml"))
                .unwrap()
                .replace(from, to)
                .into_bytes()
        };
        let octet = relabel(
            "application/pkcs7-mime; smime-type=authEnveloped-data;",
            "application/octet-stream;",
        );
        assert_eq!(classify(&octet), SmimeKind::AuthEnvelopedData);
        let untyped = relabel("smime-type=authEnveloped-data; ", "");
        assert_eq!(classify(&untyped), SmimeKind::AuthEnvelopedData);
        let bundle = format!(
            "Content-Type: application/pkcs7-mime; name=smime.p7c\r\nContent-Transfer-Encoding: base64\r\n\r\n{}",
            base64_lines(&fixture("alice-chain-p7c.der"))
        );
        assert_eq!(classify(bundle.as_bytes()), SmimeKind::CertsOnly);
    }

    #[test]
    fn content_type_parameters() {
        let ct = parse_content_type(
            "multipart/signed; protocol=\"application/pkcs7-signature\"; micalg=sha-256; boundary=\"a;b\\\"c\"",
        );
        assert_eq!(ct.mime, "multipart/signed");
        assert_eq!(ct.param("PROTOCOL"), Some("application/pkcs7-signature"));
        assert_eq!(ct.param("micalg"), Some("sha-256"));
        assert_eq!(ct.param("boundary"), Some("a;b\"c"));
        assert_eq!(ct.param("missing"), None);
    }

    #[test]
    fn multipart_split_is_byte_exact() {
        let body = b"preamble\r\n--b\r\npart one\r\n\r\n--b\r\npart two\r\n--b--\r\nepilogue";
        let parts = split_multipart(body, "b").unwrap();
        assert_eq!(parts, vec![&b"part one\r\n"[..], &b"part two"[..]]);
        // A delimiter must be a whole line: "--bx" is content.
        let body = b"--b\r\nx\r\n--bx\r\n--b--";
        assert_eq!(split_multipart(body, "b").unwrap(), vec![&b"x\r\n--bx"[..]]);
        // No close delimiter: the last part runs to the end.
        assert_eq!(
            split_multipart(b"--b\nonly", "b").unwrap(),
            vec![&b"only"[..]]
        );
        assert!(split_multipart(b"nothing", "b").is_err());
    }

    #[test]
    fn headers_unfold_and_bodies_decode() {
        let entity = b"Content-Type: application/pkcs7-mime;\r\n\tsmime-type=signed-data\r\nContent-Transfer-Encoding: BASE64\r\n\r\naGVs\r\nbG8";
        let (head, body) = split_entity(entity);
        let h = headers(head);
        assert_eq!(
            header(&h, "content-type"),
            Some("application/pkcs7-mime; smime-type=signed-data")
        );
        assert_eq!(body, b"aGVs\r\nbG8");
        assert_eq!(decode_body(entity).unwrap(), b"hello");
        let qp = b"Content-Transfer-Encoding: quoted-printable\r\n\r\nx";
        assert!(matches!(decode_body(qp), Err(Error::Unsupported(_))));
        assert_eq!(split_entity(b"\r\nbody"), (&b""[..], &b"body"[..]));
        assert_eq!(canonicalize(b"a\nb\r\nc\n"), b"a\r\nb\r\nc\r\n");
    }

    #[test]
    fn broken_signature_parts_are_errors_not_panics() {
        let w = world();
        let entity = b"Content-Type: multipart/signed; protocol=\"application/pkcs7-signature\"; boundary=b\r\n\r\n--b\r\nContent-Type: text/plain\r\n\r\nhi\r\n--b\r\nContent-Type: application/pkcs7-signature\r\nContent-Transfer-Encoding: base64\r\n\r\nbm90IGNtcw==\r\n--b--\r\n";
        let outcome = read_as(&w, "bob", entity, "a@example.org").unwrap();
        assert_eq!(signature(&outcome).state, SignatureState::Error);
        assert_eq!(outcome.content, b"Content-Type: text/plain\r\n\r\nhi");
        let keys: [DecryptionKey<'_>; 0] = [];
        let ctx = ReadContext {
            anchors: &w.anchors,
            known_certs: &w.known,
            keys: &keys,
            from: None,
            now: AT,
        };
        let one_part = b"Content-Type: multipart/signed; protocol=\"application/pkcs7-signature\"; boundary=b\r\n\r\n--b\r\nx\r\n--b--\r\n";
        assert!(read(one_part, &ctx).is_err());
        let opaque_junk = b"Content-Type: application/pkcs7-mime; smime-type=signed-data\r\nContent-Transfer-Encoding: base64\r\n\r\nbm9wZQ==\r\n";
        let outcome = read(opaque_junk, &ctx).unwrap().unwrap();
        assert_eq!(signature(&outcome).state, SignatureState::Error);
        let enveloped_junk = b"Content-Type: application/pkcs7-mime; smime-type=enveloped-data\r\nContent-Transfer-Encoding: base64\r\n\r\nbm9wZQ==\r\n";
        let outcome = read(enveloped_junk, &ctx).unwrap().unwrap();
        assert_eq!(
            outcome.status.encryption.as_ref().unwrap().state,
            DecryptionState::Failed
        );
    }
}
