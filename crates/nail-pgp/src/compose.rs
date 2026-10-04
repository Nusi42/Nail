//! Outgoing PGP/MIME (RFC 3156): sign, encrypt, sign + encrypt, with
//! header protection.
//!
//! Input is a complete RFC 5322 message as `nail-compose` (mail-builder)
//! produces it; output is the protected message, ready for the outbox.
//! The message's content (its `Content-*` header fields and body) becomes
//! the signed / encrypted MIME entity; the other header fields stay
//! outside.
//!
//! * **Signed**: `multipart/signed; protocol="application/pgp-signature";
//!   micalg=pgp-sha256` with a detached signature over the canonical
//!   (CRLF) entity.
//! * **Encrypted** (optionally signed inside, like GnuPG's
//!   `--sign --encrypt` and Thunderbird): `multipart/encrypted;
//!   protocol="application/pgp-encrypted"`, SEIPD v1 with AES-256, which
//!   every OpenPGP implementation in use reads.
//! * **Header protection** (Thunderbird's `protected-headers="v1"`, plus
//!   the `hp` parameter of RFC 9788): the protected header fields are
//!   copied into the entity; when encrypting, the outer `Subject` becomes
//!   `...` and `HP-Outer` records what was on the wire.

use pgp::composed::{ArmorOptions, DetachedSignature, MessageBuilder, SignedPublicKey};
use pgp::crypto::hash::HashAlgorithm;
use pgp::crypto::sym::SymmetricKeyAlgorithm;
use pgp::types::SigningKey;
use rand::thread_rng;

use crate::error::{Problem, Result};
use crate::key::KeyInfo;
use crate::keyring::UnlockedKey;
use crate::mime::{self, Entity, HeaderField};

/// Header fields copied into the protected part (RFC 9788 §2.1.1 and
/// Thunderbird).
pub const PROTECTED_HEADERS: &[&str] = &[
    "From",
    "To",
    "Cc",
    "Reply-To",
    "Subject",
    "Date",
    "Message-ID",
    "In-Reply-To",
    "References",
];

/// The subject shown on the wire when headers are protected.
pub const OBSCURED_SUBJECT: &str = "...";

/// How to protect an outgoing message.
#[derive(Debug)]
pub struct Protection<'a> {
    /// Sign with this own key.
    pub signer: Option<&'a UnlockedKey>,
    /// Encrypt to these keys (include the sender's own key to keep a
    /// readable copy). Empty = sign only.
    pub recipients: &'a [SignedPublicKey],
    /// Protect header fields (and hide the subject when encrypting).
    pub protect_headers: bool,
    /// `Autocrypt-Gossip` header values for the encrypted part (Autocrypt
    /// Level 1 §2.7), ignored when not encrypting.
    pub gossip: &'a [String],
    /// Evaluate key validity at this time.
    pub now: i64,
}

/// Adds a header field to a message (before the end of its header
/// block), e.g. the `Autocrypt` header. `value` may contain folding.
pub fn with_header(raw: &[u8], name: &str, value: &str) -> Vec<u8> {
    let entity = Entity::parse(raw);
    let mut fields = entity.headers.clone();
    fields.push(HeaderField::new(name, value));
    let mut out = Vec::with_capacity(raw.len() + value.len() + 64);
    mime::write_fields(&mut out, &fields);
    out.extend_from_slice(b"\r\n");
    out.extend_from_slice(&mime::to_crlf(entity.body));
    out
}

fn is_content_field(field: &HeaderField) -> bool {
    field.name.to_ascii_lowercase().starts_with("content-")
}

fn is_protected_name(name: &str) -> bool {
    PROTECTED_HEADERS
        .iter()
        .any(|p| p.eq_ignore_ascii_case(name))
}

/// Appends `; name="value"` to a raw header field.
fn add_param(field: &mut HeaderField, name: &str, value: &str) {
    field
        .raw
        .extend_from_slice(format!(";\r\n {name}=\"{value}\"").as_bytes());
}

/// Protects `raw` as described by `protection`. A signature that failed
/// on a smartcard reports the card's error (PIN not entered, card
/// removed), not rPGP's.
pub fn protect_message(raw: &[u8], protection: &Protection<'_>) -> Result<Vec<u8>> {
    protect(raw, protection).map_err(|err| {
        match protection.signer.and_then(UnlockedKey::card_error) {
            Some(card) => crate::Error::Smartcard(card),
            None => err,
        }
    })
}

fn protect(raw: &[u8], protection: &Protection<'_>) -> Result<Vec<u8>> {
    let encrypt = !protection.recipients.is_empty();
    if protection.signer.is_none() && !encrypt {
        return Err(Problem::NothingToProtect.into());
    }
    let entity = Entity::parse(raw);
    let mut outer: Vec<HeaderField> = Vec::new();
    let mut content: Vec<HeaderField> = Vec::new();
    for field in &entity.headers {
        if is_content_field(field) {
            content.push(field.clone());
        } else if !field.is("MIME-Version") {
            outer.push(field.clone());
        }
    }
    if !content.iter().any(|f| f.is("Content-Type")) {
        content.insert(
            0,
            HeaderField::new("Content-Type", "text/plain; charset=us-ascii"),
        );
    }

    // The protected entity.
    let mut inner_fields = content;
    if protection.protect_headers {
        if let Some(ct) = inner_fields.iter_mut().find(|f| f.is("Content-Type")) {
            add_param(ct, "protected-headers", "v1");
            add_param(ct, "hp", if encrypt { "cipher" } else { "clear" });
        }
        for field in outer.iter().filter(|f| is_protected_name(&f.name)) {
            inner_fields.push(field.clone());
        }
    }
    if encrypt {
        for value in protection.gossip {
            inner_fields.push(HeaderField::new("Autocrypt-Gossip", value));
        }
    }
    // Hide the subject on the wire; HP-Outer records the outer fields.
    if encrypt && protection.protect_headers {
        outer.retain(|f| !f.is("Subject"));
        outer.push(HeaderField::new("Subject", OBSCURED_SUBJECT));
        for field in outer.iter().filter(|f| is_protected_name(&f.name)) {
            inner_fields.push(HeaderField::new(
                "HP-Outer",
                &format!("{}: {}", field.name, field.value()),
            ));
        }
    }
    let mut inner = Vec::new();
    mime::write_fields(&mut inner, &inner_fields);
    inner.extend_from_slice(b"\r\n");
    inner.extend_from_slice(&mime::to_crlf(entity.body));

    let mut out = Vec::new();
    mime::write_fields(&mut out, &outer);
    out.extend_from_slice(b"MIME-Version: 1.0\r\n");
    let boundary = mime::new_boundary();
    if encrypt {
        let armored = encrypt_entity(&inner, protection)?;
        out.extend_from_slice(
            format!(
                "Content-Type: multipart/encrypted;\r\n protocol=\"application/pgp-encrypted\";\r\n boundary=\"{boundary}\"\r\n\r\n"
            )
            .as_bytes(),
        );
        out.extend_from_slice(
            format!(
                "--{boundary}\r\nContent-Type: application/pgp-encrypted\r\n\r\nVersion: 1\r\n\r\n\
                 --{boundary}\r\nContent-Type: application/octet-stream; name=\"encrypted.asc\"\r\n\
                 Content-Disposition: inline; filename=\"encrypted.asc\"\r\n\r\n"
            )
            .as_bytes(),
        );
        out.extend_from_slice(&mime::to_crlf(armored.as_bytes()));
        out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    } else {
        let Some(signer) = protection.signer else {
            return Err(Problem::NothingToProtect.into());
        };
        let signing_key = signer.signing_key(protection.now)?;
        let signature = DetachedSignature::sign_binary_data(
            thread_rng(),
            &Box::new(signing_key),
            &signer.password(),
            HashAlgorithm::Sha256,
            &inner[..],
        )?;
        let armored = signature.to_armored_string(ArmorOptions::default())?;
        out.extend_from_slice(
            format!(
                "Content-Type: multipart/signed; micalg=pgp-sha256;\r\n protocol=\"application/pgp-signature\";\r\n boundary=\"{boundary}\"\r\n\r\n"
            )
            .as_bytes(),
        );
        out.extend_from_slice(format!("--{boundary}\r\n").as_bytes());
        out.extend_from_slice(&inner);
        out.extend_from_slice(
            format!(
                "\r\n--{boundary}\r\nContent-Type: application/pgp-signature; name=\"OpenPGP_signature.asc\"\r\n\
                 Content-Disposition: attachment; filename=\"OpenPGP_signature.asc\"\r\n\r\n"
            )
            .as_bytes(),
        );
        out.extend_from_slice(&mime::to_crlf(armored.as_bytes()));
        out.extend_from_slice(format!("\r\n--{boundary}--\r\n").as_bytes());
    }
    Ok(out)
}

fn encrypt_entity(inner: &[u8], protection: &Protection<'_>) -> Result<String> {
    let mut rng = thread_rng();
    let mut builder = MessageBuilder::from_bytes("", inner.to_vec())
        .seipd_v1(&mut rng, SymmetricKeyAlgorithm::AES256);
    for key in protection.recipients {
        let info = KeyInfo::analyze(key)?;
        match info.encryption_component(protection.now)? {
            Some(index) => {
                builder.encrypt_to_key(&mut rng, &key.public_subkeys[index].key)?;
            }
            None => {
                builder.encrypt_to_key(&mut rng, &key.primary_key)?;
            }
        }
    }
    let signing_key: Option<&dyn SigningKey> = match protection.signer {
        Some(signer) => Some(signer.signing_key(protection.now)?),
        None => None,
    };
    if let (Some(signer), Some(key)) = (protection.signer, signing_key) {
        builder.sign(key, signer.password(), HashAlgorithm::Sha256);
    }
    Ok(builder.to_armored_string(&mut rng, ArmorOptions::default())?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn with_header_appends_a_field() {
        let raw = b"From: a@b\nSubject: x\n\nbody\n";
        let out = with_header(raw, "Autocrypt", "addr=a@b; keydata=AAAA");
        assert_eq!(
            out,
            b"From: a@b\r\nSubject: x\r\nAutocrypt: addr=a@b; keydata=AAAA\r\n\r\nbody\r\n"
        );
    }

    #[test]
    fn nothing_to_do_is_an_error() {
        let protection = Protection {
            signer: None,
            recipients: &[],
            protect_headers: true,
            gossip: &[],
            now: 0,
        };
        assert!(matches!(
            protect_message(b"Subject: x\r\n\r\nbody", &protection),
            Err(crate::Error::Problem(Problem::NothingToProtect))
        ));
    }
}
