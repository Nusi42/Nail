//! Incoming OpenPGP: PGP/MIME (RFC 3156) decrypt and verify, inline PGP
//! (read-only), header protection.
//!
//! [`read_message`] takes the raw message and the keyring and returns the
//! [`SecurityStatus`] plus, for protected mail, a **display message**:
//! the outer header fields (replaced by the protected ones where the
//! message carries them) around the decrypted / signed content, so the
//! core can hand it to `nail-mime` like any other message. The display
//! message holds decrypted content and must not be persisted unless the
//! account opted into indexing decrypted mail (ADR 0029).

use std::io::BufReader;

use nail_api::KeyId;
use nail_api::security::{
    CryptoTechnology, DecryptionState, EncryptionInfo, HeaderProtection, SecurityStatus,
    SignatureInfo, SignatureState, TrustLevel,
};
use nail_store::StoredKey;
use pgp::composed::DecryptionOptions;
use pgp::composed::{
    CleartextSignedMessage, Deserializable, DetachedSignature, Esk, Message, SignedPublicKey,
    TheRing,
};
use pgp::packet::{PublicKey, PublicSubkey, Signature};
use pgp::types::{KeyDetails, Password, VerifyingKey};

use crate::compose::PROTECTED_HEADERS;
use crate::error::Result;
use crate::key::{KeyInfo, hex_upper};
use crate::keyring::{CardDecryption, Keyring, effective_trust, rfc3339};
use crate::mime::{self, ContentType, Entity, HeaderField};

/// Nesting limit for multiparts and nested protection.
const MAX_DEPTH: usize = 4;

/// Inline PGP blocks handled per text part (more are left as they are).
const MAX_INLINE_BLOCKS: usize = 16;

/// What [`read_message`] found.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ReadOutcome {
    /// The end-to-end status; `None` when the message is neither signed
    /// nor encrypted with OpenPGP.
    pub status: Option<SecurityStatus>,
    /// The message to show instead of the raw one (decrypted content,
    /// protected headers applied, signature part removed); `None` = show
    /// the raw message.
    pub display: Option<Vec<u8>>,
    /// `Autocrypt-Gossip` header values found inside an encrypted part
    /// (only trustworthy there; Autocrypt Level 1 §2.7).
    pub gossip: Vec<String>,
}

/// A signing key component.
#[derive(Clone, Copy)]
enum Component<'a> {
    Primary(&'a PublicKey),
    Sub(&'a PublicSubkey, usize),
}

impl Component<'_> {
    fn as_verifying(&self) -> &dyn VerifyingKey {
        match self {
            Self::Primary(k) => *k,
            Self::Sub(k, _) => *k,
        }
    }

    fn verify_detached(&self, sig: &Signature, data: &[u8]) -> bool {
        match self {
            Self::Primary(k) => sig.verify(*k, data).is_ok(),
            Self::Sub(k, _) => sig.verify(*k, data).is_ok(),
        }
    }
}

struct Verifier<'k, 'a> {
    keyring: &'k Keyring<'a>,
    keys: Vec<(StoredKey, SignedPublicKey, KeyInfo)>,
}

impl Verifier<'_, '_> {
    fn find(&self, sig: &Signature) -> Option<(usize, Component<'_>)> {
        let fingerprints = sig.issuer_fingerprint();
        let ids = sig.issuer_key_id();
        let matches = |k: &dyn KeyDetails| {
            let fp = k.fingerprint();
            let id = k.legacy_key_id();
            fingerprints.iter().any(|f| **f == fp) || ids.iter().any(|i| **i == id)
        };
        for (n, (_, public, _)) in self.keys.iter().enumerate() {
            if matches(&public.primary_key) {
                return Some((n, Component::Primary(&public.primary_key)));
            }
            for (i, sub) in public.public_subkeys.iter().enumerate() {
                if matches(&sub.key) {
                    return Some((n, Component::Sub(&sub.key, i)));
                }
            }
        }
        None
    }

    /// Binds a signature to the displayed sender: `from` is the address
    /// the reader shows (at most one, see [`displayed_from`]). Sets
    /// `signer` (the key's address that matches, else its first),
    /// `signer_matches_from` and the key's trust **for that address**
    /// ([`Keyring::trust_for`]).
    fn bind(&self, sig: &mut SignatureInfo, from: &[String]) {
        let Some((stored, _, info)) = sig
            .key_id
            .as_ref()
            .and_then(|id| self.keys.iter().find(|(k, _, _)| &k.id == id))
        else {
            sig.signer_matches_from = false;
            return;
        };
        let matched = info.emails.iter().find(|e| from.contains(e));
        sig.signer_matches_from = matched.is_some();
        sig.signer = matched.or_else(|| info.emails.first()).cloned();
        sig.trust = match &sig.signer {
            Some(email) => self
                .keyring
                .trust_for(stored, email)
                .unwrap_or(TrustLevel::Unknown),
            // No address at all: Autocrypt vouched for none.
            None => match effective_trust(stored) {
                TrustLevel::Autocrypt | TrustLevel::AutocryptGossip => TrustLevel::Unknown,
                other => other,
            },
        };
    }

    /// Checks one signature; `verify` runs the cryptographic check with
    /// the component found; `from` as in [`Self::bind`].
    fn check(
        &self,
        sig: &Signature,
        from: &[String],
        verify: impl FnOnce(Component<'_>) -> bool,
    ) -> SignatureInfo {
        let signed_at = sig.created().map(|t| i64::from(t.as_secs()));
        let issuer = sig
            .issuer_fingerprint()
            .first()
            .map(|f| hex_upper(f.as_bytes()))
            .or_else(|| sig.issuer_key_id().first().map(|i| hex_upper(i.as_ref())));
        let Some((n, component)) = self.find(sig) else {
            return SignatureInfo {
                state: SignatureState::UnknownKey,
                signer: None,
                signer_matches_from: false,
                key_id: None,
                fingerprint: issuer,
                trust: TrustLevel::Unknown,
                signed_at: signed_at.map(rfc3339),
                chain: None,
            };
        };
        let (stored, _, info) = &self.keys[n];
        let valid = verify(component);
        let (revoked, expires_at, created_at, may_sign) = match component {
            Component::Primary(_) => (
                info.revoked,
                info.expires_at,
                info.created_at,
                info.primary_sign,
            ),
            Component::Sub(_, index) => match info.subkeys.iter().find(|s| s.index == index) {
                Some(sub) => (
                    info.revoked || sub.revoked,
                    match (info.expires_at, sub.expires_at) {
                        (Some(a), Some(b)) => Some(a.min(b)),
                        (a, b) => a.or(b),
                    },
                    sub.created_at,
                    sub.sign,
                ),
                // No valid binding: not a usable component.
                None => (info.revoked, info.expires_at, info.created_at, false),
            },
        };
        let at = signed_at.unwrap_or(0);
        let state = if !valid || !may_sign || at < created_at {
            SignatureState::Invalid
        } else if revoked {
            SignatureState::KeyRevoked
        } else if expires_at.is_some_and(|e| at >= e) {
            SignatureState::KeyExpired
        } else {
            SignatureState::Valid
        };
        let mut out = SignatureInfo {
            state,
            signer_matches_from: false,
            signer: None,
            key_id: Some(stored.id.clone()),
            fingerprint: Some(info.fingerprint.clone()),
            trust: TrustLevel::Unknown,
            signed_at: signed_at.map(rfc3339),
            chain: None,
        };
        self.bind(&mut out, from);
        out
    }
}

struct Decrypted {
    info: EncryptionInfo,
    plaintext: Option<Vec<u8>>,
    signatures: Vec<SignatureInfo>,
}

fn encryption(
    state: DecryptionState,
    key_id: Option<KeyId>,
    recipients: Vec<String>,
) -> EncryptionInfo {
    EncryptionInfo {
        state,
        key_id,
        recipients,
        algorithm: None,
    }
}

fn parse_message(data: &[u8]) -> Option<Message<'_>> {
    let text_start = find(data, b"-----BEGIN PGP MESSAGE-----");
    match text_start {
        Some(start) => Message::from_armor(BufReader::new(&data[start..]))
            .ok()
            .map(|(m, _)| m),
        None => Message::from_bytes(BufReader::new(data)).ok(),
    }
}

fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

struct Reader<'k, 'a> {
    keyring: &'k Keyring<'a>,
    verifier: Verifier<'k, 'a>,
}

impl Reader<'_, '_> {
    fn decrypt(&self, data: &[u8], from: &[String]) -> Result<Decrypted> {
        let Some(message) = parse_message(data) else {
            return Ok(Decrypted {
                info: encryption(DecryptionState::Failed, None, Vec::new()),
                plaintext: None,
                signatures: Vec::new(),
            });
        };
        let Message::Encrypted { esk, .. } = &message else {
            return Ok(Decrypted {
                info: encryption(DecryptionState::Unsupported, None, Vec::new()),
                plaintext: None,
                signatures: Vec::new(),
            });
        };
        let pkesks: Vec<_> = esk
            .iter()
            .filter_map(|e| match e {
                Esk::PublicKeyEncryptedSessionKey(p) => Some(p),
                Esk::SymKeyEncryptedSessionKey(_) => None,
            })
            .collect();
        let recipients: Vec<String> = pkesks
            .iter()
            .filter_map(|p| match p.fingerprint() {
                Ok(Some(fp)) => Some(hex_upper(fp.as_bytes())),
                _ => p
                    .id()
                    .ok()
                    .filter(|id| !id.is_wildcard())
                    .map(|id| hex_upper(id.as_ref())),
            })
            .collect();
        let anonymous = pkesks
            .iter()
            .any(|p| p.id().is_ok_and(pgp::types::KeyId::is_wildcard));

        // Own keys the message is encrypted to.
        let mut candidates = Vec::new();
        for (stored, secret) in self.keyring.own_secret_keys()? {
            let addressed = anonymous
                || pkesks.iter().any(|p| {
                    p.match_identity(secret.primary_key.public_key())
                        || secret
                            .secret_subkeys
                            .iter()
                            .any(|s| p.match_identity(s.key.public_key()))
                });
            if addressed {
                candidates.push((stored, secret));
            }
        }
        // M4c WP122: own keys on a smartcard (RSA session keys are
        // decrypted on the card).
        let mut on_card = None;
        if candidates.is_empty() {
            match self.keyring.card_session_key(&pkesks)? {
                None => {
                    return Ok(Decrypted {
                        info: encryption(DecryptionState::NoKey, None, recipients),
                        plaintext: None,
                        signatures: Vec::new(),
                    });
                }
                Some(CardDecryption::Failed(stored, err)) => {
                    // The card is missing or its PIN was not entered: like
                    // a locked key, the user can try again.
                    let state =
                        if err.kind == nail_api::smartcard::SmartcardError::MechanismUnsupported {
                            DecryptionState::Unsupported
                        } else {
                            DecryptionState::Locked
                        };
                    return Ok(Decrypted {
                        info: encryption(state, Some(stored.id), recipients),
                        plaintext: None,
                        signatures: Vec::new(),
                    });
                }
                Some(CardDecryption::Session(stored, session)) => {
                    let opened =
                        parse_message(data).and_then(|m| m.decrypt_with_session_key(session).ok());
                    match opened {
                        Some(opened) => on_card = Some((opened, stored.id)),
                        None => {
                            return Ok(Decrypted {
                                info: encryption(
                                    DecryptionState::Failed,
                                    Some(stored.id),
                                    recipients,
                                ),
                                plaintext: None,
                                signatures: Vec::new(),
                            });
                        }
                    }
                }
            }
        }
        let (mut message, key_id) = if let Some(found) = on_card {
            found
        } else {
            let mut unlocked = Vec::new();
            let mut locked = Vec::new();
            for (stored, secret) in candidates {
                match self.keyring.try_password(&stored)? {
                    Some(password) => {
                        unlocked.push((stored, secret, Password::from(password.as_str())));
                    }
                    None => locked.push(stored),
                }
            }
            if unlocked.is_empty() {
                return Ok(Decrypted {
                    info: encryption(
                        DecryptionState::Locked,
                        locked.first().map(|s| s.id.clone()),
                        recipients,
                    ),
                    plaintext: None,
                    signatures: Vec::new(),
                });
            }
            // One key at a time, so the key that worked is known (rPGP's ring
            // result is overwritten per session key packet).
            let mut decrypted = None;
            let mut first = Some(message);
            for (stored, secret, password) in &unlocked {
                let Some(attempt) = first.take().or_else(|| parse_message(data)) else {
                    break;
                };
                let ring = TheRing {
                    secret_keys: vec![secret],
                    key_passwords: vec![password],
                    decrypt_options: DecryptionOptions::new().enable_gnupg_aead(),
                    ..TheRing::default()
                };
                if let Ok((message, _)) = attempt.decrypt_the_ring(ring, true) {
                    decrypted = Some((message, stored.id.clone()));
                    break;
                }
            }
            let Some(found) = decrypted else {
                let state = if locked.is_empty() {
                    DecryptionState::Failed
                } else {
                    DecryptionState::Locked
                };
                let key_id = locked.first().map(|s| s.id.clone());
                return Ok(Decrypted {
                    info: encryption(state, key_id, recipients),
                    plaintext: None,
                    signatures: Vec::new(),
                });
            };
            found
        };
        let key_id = Some(key_id);
        let failed = |recipients: Vec<String>, key_id: Option<KeyId>| Decrypted {
            info: encryption(DecryptionState::Failed, key_id, recipients),
            plaintext: None,
            signatures: Vec::new(),
        };
        for _ in 0..MAX_DEPTH {
            if !message.is_compressed() {
                break;
            }
            message = match message.decompress() {
                Ok(m) => m,
                Err(_) => return Ok(failed(recipients, key_id)),
            };
        }
        // Reading to the end checks the integrity tag (MDC / AEAD).
        let Ok(plaintext) = message.as_data_vec() else {
            return Ok(failed(recipients, key_id));
        };
        let mut signatures = Vec::new();
        if let Message::Signed { reader, .. } = &message {
            for index in 0..reader.num_signatures() {
                let Some(sig) = reader.signature(index) else {
                    continue;
                };
                let info = self.verifier.check(sig, from, |component| {
                    message
                        .verify_nested_explicit(index, component.as_verifying())
                        .is_ok()
                });
                signatures.push(info);
            }
        }
        Ok(Decrypted {
            info: encryption(DecryptionState::Decrypted, key_id, recipients),
            plaintext: Some(plaintext),
            signatures,
        })
    }

    fn verify_detached(
        &self,
        signed: &[u8],
        signature_part: &[u8],
        from: &[String],
    ) -> Vec<SignatureInfo> {
        let data = mime::decoded_bytes(signature_part).unwrap_or_default();
        let parsed: Vec<DetachedSignature> = match find(&data, b"-----BEGIN PGP SIGNATURE-----") {
            Some(start) => DetachedSignature::from_armor_many(BufReader::new(&data[start..]))
                .map(|(iter, _)| iter.filter_map(std::result::Result::ok).collect())
                .unwrap_or_default(),
            None => DetachedSignature::from_bytes_many(BufReader::new(&data[..]))
                .map(|iter| iter.filter_map(std::result::Result::ok).collect())
                .unwrap_or_default(),
        };
        if parsed.is_empty() {
            return vec![malformed_signature()];
        }
        parsed
            .iter()
            .map(|d| {
                self.verifier.check(&d.signature, from, |c| {
                    c.verify_detached(&d.signature, signed)
                })
            })
            .collect()
    }
}

fn malformed_signature() -> SignatureInfo {
    SignatureInfo {
        state: SignatureState::Error,
        signer: None,
        signer_matches_from: false,
        key_id: None,
        fingerprint: None,
        trust: TrustLevel::Unknown,
        signed_at: None,
        chain: None,
    }
}

fn status(signatures: Vec<SignatureInfo>, encryption: Option<EncryptionInfo>) -> SecurityStatus {
    SecurityStatus {
        technology: CryptoTechnology::OpenPgp,
        signatures,
        encryption,
        header_protection: HeaderProtection::None,
        partial: false,
        inline_pgp: false,
    }
}

/// Header protection of an inner entity against the outer header.
fn header_protection(
    outer: &Entity<'_>,
    inner: &Entity<'_>,
    encrypted: bool,
) -> (HeaderProtection, Vec<HeaderField>) {
    let ct = inner.content_type();
    if ct.param("protected-headers").is_none() && ct.param("hp").is_none() {
        return (HeaderProtection::None, Vec::new());
    }
    let protected: Vec<HeaderField> = inner
        .headers
        .iter()
        .filter(|h| PROTECTED_HEADERS.iter().any(|p| h.is(p)))
        .cloned()
        .collect();
    if protected.is_empty() {
        return (HeaderProtection::None, Vec::new());
    }
    let addresses = |entity: &Entity<'_>, name: &str| {
        let mut list = entity
            .header_value(name)
            .map(|v| mime::addresses(&v, name))
            .unwrap_or_default();
        list.sort();
        list
    };
    let mut mismatch = false;
    for name in ["From", "To", "Cc"] {
        let (o, i) = (outer.header(name), inner.header(name));
        if let (Some(_), Some(_)) = (o, i)
            && addresses(outer, name) != addresses(inner, name)
        {
            mismatch = true;
        }
    }
    if !encrypted
        && let (Some(o), Some(i)) = (outer.header_value("Subject"), inner.header_value("Subject"))
        && !o.contains(i.as_str())
    {
        mismatch = true;
    }
    let state = if mismatch {
        HeaderProtection::Mismatch
    } else {
        HeaderProtection::Protected
    };
    (state, protected)
}

/// Outer header fields (protected ones replaced) + the inner entity's
/// content.
fn display(outer: &Entity<'_>, inner: &Entity<'_>, protected: &[HeaderField]) -> Vec<u8> {
    let mut fields: Vec<HeaderField> = outer
        .headers
        .iter()
        .filter(|h| {
            !h.name.to_ascii_lowercase().starts_with("content-")
                && !h.is("MIME-Version")
                && !protected.iter().any(|p| p.is(&h.name))
        })
        .cloned()
        .collect();
    fields.extend(protected.iter().cloned());
    fields.push(HeaderField::new("MIME-Version", "1.0"));
    fields.extend(
        inner
            .headers
            .iter()
            .filter(|h| h.name.to_ascii_lowercase().starts_with("content-"))
            .cloned(),
    );
    let mut out = Vec::new();
    mime::write_fields(&mut out, &fields);
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(inner.body);
    out
}

/// The sender address the reader shows, to bind signers to: the From of
/// the protected header fields when the message carries protected headers
/// (they replace the outer ones in the display), else the outer From.
/// Only the first address counts (as for S/MIME). A `From` inside the
/// signed or encrypted part without a header-protection marker is never
/// used: it is not what the reader shows.
fn displayed_from(outer: &Entity<'_>, protected: &[HeaderField]) -> Vec<String> {
    let value = match protected.iter().find(|h| h.is("From")) {
        Some(field) => Some(field.value()),
        None => outer.header_value("From"),
    };
    value
        .map(|v| mime::addresses(&v, "From"))
        .unwrap_or_default()
        .into_iter()
        .take(1)
        .collect()
}

fn gossip_of(inner: &Entity<'_>) -> Vec<String> {
    inner
        .headers
        .iter()
        .filter(|h| h.is("Autocrypt-Gossip"))
        .map(HeaderField::value)
        .collect()
}

/// Reads a message: decrypts and verifies PGP/MIME, reads inline PGP.
pub fn read_message(keyring: &Keyring<'_>, raw: &[u8]) -> Result<ReadOutcome> {
    let reader = Reader {
        keyring,
        verifier: Verifier {
            keyring,
            keys: keyring.all_public()?,
        },
    };
    let outer = Entity::parse(raw);
    let ct = outer.content_type();
    if is_encrypted(&ct) {
        return reader.read_encrypted(&outer, &ct);
    }
    if is_signed(&ct) {
        return Ok(reader.read_signed(&outer, &outer, &ct, None));
    }
    if ct.is("multipart/mixed") {
        // A signed part followed by a footer (mailing lists): partial.
        if let Some(boundary) = ct.param("boundary")
            && let Some(parts) = mime::split_multipart(outer.body, boundary)
            && let Some(first) = parts.parts.first()
        {
            let child = Entity::parse(first);
            let child_ct = child.content_type();
            if is_signed(&child_ct) {
                let mut outcome = reader.read_signed(&outer, &child, &child_ct, None);
                // The raw message is shown: bind signers to its From.
                let from = displayed_from(&outer, &[]);
                if let Some(status) = &mut outcome.status {
                    status.partial = parts.parts.len() > 1;
                    for s in &mut status.signatures {
                        reader.verifier.bind(s, &from);
                    }
                }
                outcome.display = None;
                return Ok(outcome);
            }
        }
    }
    reader.read_inline(&outer, raw)
}

fn is_encrypted(ct: &ContentType) -> bool {
    ct.is("multipart/encrypted")
        && ct
            .param("protocol")
            .is_some_and(|p| p.eq_ignore_ascii_case("application/pgp-encrypted"))
}

fn is_signed(ct: &ContentType) -> bool {
    ct.is("multipart/signed")
        && ct
            .param("protocol")
            .is_some_and(|p| p.eq_ignore_ascii_case("application/pgp-signature"))
}

impl Reader<'_, '_> {
    fn read_encrypted(&self, outer: &Entity<'_>, ct: &ContentType) -> Result<ReadOutcome> {
        let parts = ct
            .param("boundary")
            .and_then(|b| mime::split_multipart(outer.body, b));
        let Some(payload) = parts.as_ref().and_then(|p| p.parts.get(1)) else {
            return Ok(ReadOutcome {
                status: Some(status(
                    Vec::new(),
                    Some(encryption(DecryptionState::Failed, None, Vec::new())),
                )),
                ..ReadOutcome::default()
            });
        };
        let payload = Entity::parse(payload);
        let from = displayed_from(outer, &[]);
        let decrypted = self.decrypt(payload.body, &from)?;
        let Some(plaintext) = decrypted.plaintext else {
            return Ok(ReadOutcome {
                status: Some(status(decrypted.signatures, Some(decrypted.info))),
                ..ReadOutcome::default()
            });
        };
        let inner = Entity::parse(&plaintext);
        let inner_ct = inner.content_type();
        // Sign-then-encrypt as nested multipart/signed (RFC 3156 §6.1).
        if is_signed(&inner_ct) {
            let mut outcome = self.read_signed(outer, &inner, &inner_ct, Some(decrypted.info));
            // The display carries the From the reader shows.
            let shown = outcome.display.as_deref().map_or_else(
                || displayed_from(outer, &[]),
                |d| displayed_from(&Entity::parse(d), &[]),
            );
            if let Some(status) = &mut outcome.status {
                let mut signatures = decrypted.signatures;
                for s in &mut signatures {
                    self.verifier.bind(s, &shown);
                }
                signatures.append(&mut status.signatures);
                status.signatures = signatures;
            }
            return Ok(outcome);
        }
        let (protection, protected) = header_protection(outer, &inner, true);
        // Re-bind the signers to the From the reader shows.
        let from = displayed_from(outer, &protected);
        let signatures = decrypted
            .signatures
            .into_iter()
            .map(|mut s| {
                self.verifier.bind(&mut s, &from);
                s
            })
            .collect();
        let mut st = status(signatures, Some(decrypted.info));
        st.header_protection = protection;
        Ok(ReadOutcome {
            status: Some(st),
            display: Some(display(outer, &inner, &protected)),
            gossip: gossip_of(&inner),
        })
    }

    /// `signed_entity` is the `multipart/signed` entity (the message
    /// itself, or the decrypted content).
    fn read_signed(
        &self,
        outer: &Entity<'_>,
        signed_entity: &Entity<'_>,
        ct: &ContentType,
        encryption: Option<EncryptionInfo>,
    ) -> ReadOutcome {
        let encrypted = encryption.is_some();
        let parts = ct
            .param("boundary")
            .and_then(|b| mime::split_multipart(signed_entity.body, b));
        let (Some(content), Some(signature)) = (
            parts.as_ref().and_then(|p| p.parts.first()),
            parts.as_ref().and_then(|p| p.parts.get(1)),
        ) else {
            return ReadOutcome {
                status: Some(status(vec![malformed_signature()], encryption)),
                ..ReadOutcome::default()
            };
        };
        let inner = Entity::parse(content);
        let (protection, protected) = header_protection(outer, &inner, encrypted);
        let from = displayed_from(outer, &protected);
        let signatures = self.verify_detached(content, signature, &from);
        let mut st = status(signatures, encryption);
        st.header_protection = protection;
        ReadOutcome {
            status: Some(st),
            display: Some(display(outer, &inner, &protected)),
            gossip: if encrypted {
                gossip_of(&inner)
            } else {
                Vec::new()
            },
        }
    }

    /// Inline PGP in the text/plain root or first-level text/plain parts.
    fn read_inline(&self, outer: &Entity<'_>, raw: &[u8]) -> Result<ReadOutcome> {
        let from = displayed_from(outer, &[]);
        let mut found = InlineFindings::default();
        let rebuilt = self.rewrite_inline(raw, &from, &mut found, 0)?;
        if !found.any {
            return Ok(ReadOutcome::default());
        }
        let mut st = status(found.signatures, found.encryption);
        st.inline_pgp = true;
        st.partial = found.partial;
        Ok(ReadOutcome {
            status: Some(st),
            display: rebuilt,
            gossip: Vec::new(),
        })
    }

    /// Returns the entity with its inline PGP blocks replaced, or `None`
    /// when nothing changed.
    fn rewrite_inline(
        &self,
        entity_raw: &[u8],
        from: &[String],
        found: &mut InlineFindings,
        depth: usize,
    ) -> Result<Option<Vec<u8>>> {
        let entity = Entity::parse(entity_raw);
        let ct = entity.content_type();
        if ct.is("text/plain") {
            let is_attachment = entity
                .header_value("Content-Disposition")
                .is_some_and(|d| d.to_ascii_lowercase().starts_with("attachment"));
            if is_attachment {
                return Ok(None);
            }
            let Some(text) = mime::decoded_text(entity_raw) else {
                return Ok(None);
            };
            let Some(new_text) = self.inline_text(&text, from, found)? else {
                return Ok(None);
            };
            let mut fields: Vec<HeaderField> = entity
                .headers
                .iter()
                .filter(|h| !h.is("Content-Type") && !h.is("Content-Transfer-Encoding"))
                .cloned()
                .collect();
            fields.push(HeaderField::new(
                "Content-Type",
                "text/plain; charset=utf-8",
            ));
            fields.push(HeaderField::new("Content-Transfer-Encoding", "8bit"));
            let mut out = Vec::new();
            mime::write_fields(&mut out, &fields);
            out.extend_from_slice(b"\r\n");
            out.extend_from_slice(&mime::to_crlf(new_text.as_bytes()));
            return Ok(Some(out));
        }
        if depth >= MAX_DEPTH || !(ct.is("multipart/mixed") || ct.is("multipart/alternative")) {
            return Ok(None);
        }
        let Some(boundary) = ct.param("boundary") else {
            return Ok(None);
        };
        let Some(parts) = mime::split_multipart(entity.body, boundary) else {
            return Ok(None);
        };
        let mut changed = false;
        let mut new_parts = Vec::new();
        for part in &parts.parts {
            match self.rewrite_inline(part, from, found, depth + 1)? {
                Some(new) => {
                    changed = true;
                    new_parts.push(new);
                }
                None => new_parts.push(part.to_vec()),
            }
        }
        if !changed {
            return Ok(None);
        }
        // Other parts (HTML, attachments) next to a protected block are not
        // protected: never show the whole message as signed / encrypted.
        if parts.parts.len() > 1 {
            found.partial = true;
        }
        let mut out = Vec::new();
        mime::write_fields(&mut out, &entity.headers);
        out.extend_from_slice(b"\r\n");
        if !parts.preamble.is_empty() {
            out.extend_from_slice(parts.preamble);
            out.extend_from_slice(b"\r\n");
        }
        for part in new_parts {
            out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
            out.extend_from_slice(&part);
            out.extend_from_slice(b"\r\n");
        }
        out.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
        Ok(Some(out))
    }

    /// Replaces the armored blocks of a text; `None` when there are none.
    fn inline_text(
        &self,
        text: &str,
        from: &[String],
        found: &mut InlineFindings,
    ) -> Result<Option<String>> {
        const MESSAGE: &str = "-----BEGIN PGP MESSAGE-----";
        const MESSAGE_END: &str = "-----END PGP MESSAGE-----";
        const SIGNED: &str = "-----BEGIN PGP SIGNED MESSAGE-----";
        const SIGNATURE_END: &str = "-----END PGP SIGNATURE-----";
        let mut out = String::new();
        let mut rest = text;
        let mut blocks = 0;
        let mut outside = String::new();
        loop {
            let next = [(rest.find(MESSAGE), true), (rest.find(SIGNED), false)]
                .into_iter()
                .filter_map(|(pos, enc)| pos.map(|p| (p, enc)))
                .min_by_key(|(p, _)| *p);
            let Some((start, encrypted)) = next else {
                break;
            };
            if blocks >= MAX_INLINE_BLOCKS {
                break;
            }
            let end_marker = if encrypted {
                MESSAGE_END
            } else {
                SIGNATURE_END
            };
            let Some(end_rel) = rest[start..].find(end_marker) else {
                break;
            };
            let end = start + end_rel + end_marker.len();
            let block = &rest[start..end];
            out.push_str(&rest[..start]);
            outside.push_str(&rest[..start]);
            blocks += 1;
            if encrypted {
                let decrypted = self.decrypt(block.as_bytes(), from)?;
                // Several blocks: report the first one that did not decrypt.
                let keep_old = found
                    .encryption
                    .as_ref()
                    .is_some_and(|e| e.state != DecryptionState::Decrypted);
                if !keep_old {
                    found.encryption = Some(decrypted.info);
                }
                found.signatures.extend(decrypted.signatures);
                match decrypted.plaintext {
                    Some(plain) => out.push_str(&String::from_utf8_lossy(&plain)),
                    None => out.push_str(block),
                }
            } else if let Ok((message, _)) = CleartextSignedMessage::from_string(block) {
                let signed_text = message.signed_text();
                for sig in message.signatures() {
                    found.signatures.push(self.verifier.check(sig, from, |c| {
                        c.verify_detached(sig, signed_text.as_bytes())
                    }));
                }
                out.push_str(&signed_text.replace("\r\n", "\n"));
            } else {
                found.signatures.push(malformed_signature());
                out.push_str(block);
            }
            rest = &rest[end..];
        }
        if blocks == 0 {
            return Ok(None);
        }
        out.push_str(rest);
        outside.push_str(rest);
        found.any = true;
        if blocks > 1 || !outside.trim().is_empty() {
            found.partial = true;
        }
        Ok(Some(out))
    }
}

#[derive(Default)]
struct InlineFindings {
    any: bool,
    partial: bool,
    signatures: Vec<SignatureInfo>,
    encryption: Option<EncryptionInfo>,
}
