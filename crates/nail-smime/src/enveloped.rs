//! CMS `EnvelopedData` (RFC 5652 section 6, AES-CBC) and
//! `AuthEnvelopedData` (RFC 5083, AES-GCM): encryption and decryption.
//!
//! Recipients: RSA key transport (RSAES-PKCS1-v1_5 when encrypting, for
//! every client's sake; PKCS1-v1_5 and RSAES-OAEP when decrypting) and
//! ECDH key agreement (RFC 5753 `dhSinglePass-stdDH-sha*kdf-scheme` with
//! AES key wrap). The default content cipher is AES-256-GCM (RFC 8551
//! section 2.7); AES-CBC stays available for old clients.

use aes::cipher::generic_array::GenericArray;
use aes_gcm::aead::{Aead, KeyInit, Payload};
use cbc::cipher::block_padding::Pkcs7;
use cbc::cipher::{BlockDecryptMut, BlockEncryptMut, KeyIvInit};
use cms::content_info::{CmsVersion, ContentInfo};
use cms::enveloped_data::{
    EncryptedContentInfo, EnvelopedData, KeyAgreeRecipientIdentifier, KeyAgreeRecipientInfo,
    KeyTransRecipientInfo, OriginatorIdentifierOrKey, OriginatorInfo, OriginatorPublicKey,
    RecipientEncryptedKey, RecipientIdentifier, RecipientInfo, RecipientInfos,
};
use der::asn1::{BitString, OctetString, SetOfVec};
use der::{Any, Decode, Encode, Sequence};
use rand::RngCore;
use rand::rngs::OsRng;
use rsa::Pkcs1v15Encrypt;
use spki::AlgorithmIdentifierOwned;
use x509_cert::attr::Attributes;
use zeroize::Zeroizing;

use crate::ber;
use crate::cert::{Cert, hex_upper};
use crate::error::{Error, Result};
use crate::keys::{HashAlg, PrivateKeyOps, PublicKey, RsaPadding};
use crate::oids;

/// A content-encryption algorithm Nail can produce.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum ContentCipher {
    /// AES-128-GCM (`AuthEnvelopedData`).
    Aes128Gcm,
    /// AES-256-GCM (`AuthEnvelopedData`, the default).
    #[default]
    Aes256Gcm,
    /// AES-128-CBC (`EnvelopedData`, for clients without GCM).
    Aes128Cbc,
    /// AES-256-CBC (`EnvelopedData`, for clients without GCM).
    Aes256Cbc,
}

impl ContentCipher {
    fn key_len(self) -> usize {
        match self {
            Self::Aes128Gcm | Self::Aes128Cbc => 16,
            Self::Aes256Gcm | Self::Aes256Cbc => 32,
        }
    }

    /// Whether this is an authenticated cipher (`AuthEnvelopedData`).
    pub fn is_authenticated(self) -> bool {
        matches!(self, Self::Aes128Gcm | Self::Aes256Gcm)
    }

    /// The `smime-type` of the resulting `application/pkcs7-mime` part.
    pub fn smime_type(self) -> &'static str {
        if self.is_authenticated() {
            "authEnveloped-data"
        } else {
            "enveloped-data"
        }
    }
}

/// `AuthEnvelopedData` (RFC 5083 section 2.1).
#[derive(Clone, Debug, Eq, PartialEq, Sequence)]
struct AuthEnvelopedData {
    version: CmsVersion,
    #[asn1(
        context_specific = "0",
        tag_mode = "IMPLICIT",
        constructed = "true",
        optional = "true"
    )]
    originator_info: Option<OriginatorInfo>,
    recip_infos: RecipientInfos,
    auth_encrypted_content: EncryptedContentInfo,
    #[asn1(
        context_specific = "1",
        tag_mode = "IMPLICIT",
        constructed = "true",
        optional = "true"
    )]
    auth_attrs: Option<Attributes>,
    mac: OctetString,
    #[asn1(
        context_specific = "2",
        tag_mode = "IMPLICIT",
        constructed = "true",
        optional = "true"
    )]
    unauth_attrs: Option<Attributes>,
}

fn default_icv_len() -> u8 {
    12
}

/// `GCMParameters` (RFC 5084 section 3.2).
#[derive(Clone, Debug, Eq, PartialEq, Sequence)]
struct GcmParameters {
    nonce: OctetString,
    #[asn1(default = "default_icv_len")]
    icv_len: u8,
}

/// `ECC-CMS-SharedInfo` (RFC 5753 section 7.2).
#[derive(Clone, Debug, Eq, PartialEq, Sequence)]
#[expect(
    clippy::struct_field_names,
    reason = "the field names follow the ASN.1 module of RFC 5753 (keyInfo, entityUInfo, suppPubInfo)"
)]
struct EccCmsSharedInfo {
    key_info: AlgorithmIdentifierOwned,
    #[asn1(context_specific = "0", tag_mode = "EXPLICIT", optional = "true")]
    entity_u_info: Option<OctetString>,
    #[asn1(context_specific = "2", tag_mode = "EXPLICIT")]
    supp_pub_info: OctetString,
}

/// Encrypts `plaintext` (a MIME entity) to every certificate; returns the
/// DER `ContentInfo`. Include the sender's own certificate to keep a
/// readable copy.
pub fn encrypt(plaintext: &[u8], recipients: &[Cert], cipher: ContentCipher) -> Result<Vec<u8>> {
    if recipients.is_empty() {
        return Err(Error::NoKey("no recipients".into()));
    }
    let mut cek = Zeroizing::new(vec![0u8; cipher.key_len()]);
    OsRng.fill_bytes(&mut cek);
    let mut infos = Vec::with_capacity(recipients.len());
    for cert in recipients {
        infos.push(recipient_info(cert, &cek, cipher)?);
    }
    let any_kari = infos.iter().any(|i| matches!(i, RecipientInfo::Kari(_)));
    let recip_infos = RecipientInfos(SetOfVec::try_from(infos)?);

    let (content_type, content) = if cipher.is_authenticated() {
        let mut nonce = [0u8; 12];
        OsRng.fill_bytes(&mut nonce);
        let params = GcmParameters {
            nonce: OctetString::new(nonce.to_vec())?,
            icv_len: 16,
        };
        let sealed = gcm_seal(cipher, &cek, &nonce, plaintext)?;
        let (ciphertext, tag) = sealed.split_at(sealed.len() - 16);
        let oid = match cipher {
            ContentCipher::Aes128Gcm => oids::AES128_GCM,
            _ => oids::AES256_GCM,
        };
        let data = AuthEnvelopedData {
            version: CmsVersion::V0,
            originator_info: None,
            recip_infos,
            auth_encrypted_content: EncryptedContentInfo {
                content_type: oids::ID_DATA,
                content_enc_alg: AlgorithmIdentifierOwned {
                    oid,
                    parameters: Some(Any::encode_from(&params)?),
                },
                encrypted_content: Some(OctetString::new(ciphertext.to_vec())?),
            },
            auth_attrs: None,
            mac: OctetString::new(tag.to_vec())?,
            unauth_attrs: None,
        };
        (oids::ID_AUTH_ENVELOPED_DATA, Any::encode_from(&data)?)
    } else {
        let mut iv = [0u8; 16];
        OsRng.fill_bytes(&mut iv);
        let (oid, ciphertext) = match cipher {
            ContentCipher::Aes128Cbc => (
                oids::AES128_CBC,
                cbc::Encryptor::<aes::Aes128>::new_from_slices(&cek, &iv)
                    .map_err(|_| Error::Crypto("bad key length".into()))?
                    .encrypt_padded_vec_mut::<Pkcs7>(plaintext),
            ),
            _ => (
                oids::AES256_CBC,
                cbc::Encryptor::<aes::Aes256>::new_from_slices(&cek, &iv)
                    .map_err(|_| Error::Crypto("bad key length".into()))?
                    .encrypt_padded_vec_mut::<Pkcs7>(plaintext),
            ),
        };
        let data = EnvelopedData {
            // RFC 5652 section 6.1: 0 when every recipient is a v0 KTRI.
            version: if any_kari {
                CmsVersion::V2
            } else {
                CmsVersion::V0
            },
            originator_info: None,
            recip_infos,
            encrypted_content: EncryptedContentInfo {
                content_type: oids::ID_DATA,
                content_enc_alg: AlgorithmIdentifierOwned {
                    oid,
                    parameters: Some(Any::encode_from(&OctetString::new(iv.to_vec())?)?),
                },
                encrypted_content: Some(OctetString::new(ciphertext)?),
            },
            unprotected_attrs: None,
        };
        (oids::ID_ENVELOPED_DATA, Any::encode_from(&data)?)
    };
    Ok(ContentInfo {
        content_type,
        content,
    }
    .to_der()?)
}

fn recipient_info(cert: &Cert, cek: &[u8], cipher: ContentCipher) -> Result<RecipientInfo> {
    if !cert.can_encrypt() {
        return Err(Error::NoKey(format!(
            "certificate {} cannot be used for encryption",
            cert.subject()
        )));
    }
    match cert.public_key()? {
        PublicKey::Rsa(key) => {
            let enc_key = key
                .encrypt(&mut OsRng, Pkcs1v15Encrypt, cek)
                .map_err(|e| Error::Crypto(format!("key transport failed: {e}")))?;
            Ok(RecipientInfo::Ktri(KeyTransRecipientInfo {
                version: CmsVersion::V0,
                rid: RecipientIdentifier::IssuerAndSerialNumber(cert.issuer_and_serial()),
                key_enc_alg: AlgorithmIdentifierOwned {
                    oid: oids::RSA_ENCRYPTION,
                    parameters: Some(der::asn1::Null.into()),
                },
                enc_key: OctetString::new(enc_key)?,
            }))
        }
        public @ (PublicKey::P256(_) | PublicKey::P384(_)) => {
            let (kdf_oid, kdf_hash) = match public {
                PublicKey::P384(_) => (oids::ECDH_STD_SHA384_KDF, HashAlg::Sha384),
                _ => (oids::ECDH_STD_SHA256_KDF, HashAlg::Sha256),
            };
            let wrap = AlgorithmIdentifierOwned {
                oid: if cipher.key_len() == 16 {
                    oids::AES128_WRAP
                } else {
                    oids::AES256_WRAP
                },
                parameters: None,
            };
            let (ephemeral_point, z) = ephemeral_agreement(&public)?;
            let kek = kek_from_shared(kdf_hash, &z, &wrap, None)?;
            let enc_key = aes_wrap(&kek, cek)?;
            Ok(RecipientInfo::Kari(KeyAgreeRecipientInfo {
                version: CmsVersion::V3,
                originator: OriginatorIdentifierOrKey::OriginatorKey(OriginatorPublicKey {
                    algorithm: AlgorithmIdentifierOwned {
                        oid: oids::EC_PUBLIC_KEY,
                        parameters: None,
                    },
                    public_key: BitString::from_bytes(&ephemeral_point)?,
                }),
                ukm: None,
                key_enc_alg: AlgorithmIdentifierOwned {
                    oid: kdf_oid,
                    parameters: Some(Any::encode_from(&wrap)?),
                },
                recipient_enc_keys: vec![RecipientEncryptedKey {
                    rid: KeyAgreeRecipientIdentifier::IssuerAndSerialNumber(
                        cert.issuer_and_serial(),
                    ),
                    enc_key: OctetString::new(enc_key)?,
                }],
            }))
        }
    }
}

/// A fresh ephemeral key on the recipient's curve: its point and the
/// shared secret.
fn ephemeral_agreement(recipient: &PublicKey) -> Result<(Vec<u8>, Zeroizing<Vec<u8>>)> {
    use p256::elliptic_curve::sec1::ToEncodedPoint;
    match recipient {
        PublicKey::P256(peer) => {
            let secret = p256::ecdh::EphemeralSecret::random(&mut OsRng);
            let point = secret
                .public_key()
                .to_encoded_point(false)
                .as_bytes()
                .to_vec();
            let shared = secret.diffie_hellman(peer);
            Ok((point, Zeroizing::new(shared.raw_secret_bytes().to_vec())))
        }
        PublicKey::P384(peer) => {
            let secret = p384::ecdh::EphemeralSecret::random(&mut OsRng);
            let point = secret
                .public_key()
                .to_encoded_point(false)
                .as_bytes()
                .to_vec();
            let shared = secret.diffie_hellman(peer);
            Ok((point, Zeroizing::new(shared.raw_secret_bytes().to_vec())))
        }
        PublicKey::Rsa(_) => Err(Error::Crypto("not an EC key".into())),
    }
}

/// ANSI X9.63 KDF over `ECC-CMS-SharedInfo` (RFC 5753 section 7.2).
fn kek_from_shared(
    hash: HashAlg,
    z: &[u8],
    wrap: &AlgorithmIdentifierOwned,
    ukm: Option<&OctetString>,
) -> Result<Zeroizing<Vec<u8>>> {
    let key_len = wrap_key_len(wrap)?;
    let shared_info = EccCmsSharedInfo {
        key_info: wrap.clone(),
        entity_u_info: ukm.cloned(),
        supp_pub_info: OctetString::new(
            u32::try_from(key_len * 8)
                .unwrap_or(0)
                .to_be_bytes()
                .to_vec(),
        )?,
    }
    .to_der()?;
    Ok(x963_kdf(hash, z, &shared_info, key_len))
}

/// ANSI X9.63 key derivation.
pub(crate) fn x963_kdf(
    hash: HashAlg,
    z: &[u8],
    shared_info: &[u8],
    len: usize,
) -> Zeroizing<Vec<u8>> {
    let mut out = Zeroizing::new(Vec::with_capacity(len + 64));
    let mut counter: u32 = 1;
    while out.len() < len {
        let mut input = Zeroizing::new(Vec::with_capacity(z.len() + 4 + shared_info.len()));
        input.extend_from_slice(z);
        input.extend_from_slice(&counter.to_be_bytes());
        input.extend_from_slice(shared_info);
        out.extend_from_slice(&hash.digest(&input));
        counter += 1;
    }
    out.truncate(len);
    out
}

fn wrap_key_len(wrap: &AlgorithmIdentifierOwned) -> Result<usize> {
    match wrap.oid {
        oids::AES128_WRAP => Ok(16),
        oids::AES192_WRAP => Ok(24),
        oids::AES256_WRAP => Ok(32),
        other => Err(Error::Unsupported(format!("key wrap {other}"))),
    }
}

fn aes_wrap(kek: &[u8], cek: &[u8]) -> Result<Vec<u8>> {
    let failed = |_| Error::Crypto("key wrap failed".into());
    match kek.len() {
        16 => aes_kw::KekAes128::new(GenericArray::from_slice(kek))
            .wrap_vec(cek)
            .map_err(failed),
        24 => aes_kw::KekAes192::new(GenericArray::from_slice(kek))
            .wrap_vec(cek)
            .map_err(failed),
        32 => aes_kw::KekAes256::new(GenericArray::from_slice(kek))
            .wrap_vec(cek)
            .map_err(failed),
        _ => Err(Error::Crypto("bad key-encryption key length".into())),
    }
}

fn aes_unwrap(kek: &[u8], wrapped: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let failed = |_| Error::Crypto("key unwrap failed (wrong key or damaged data)".into());
    let plain = match kek.len() {
        16 => aes_kw::KekAes128::new(GenericArray::from_slice(kek)).unwrap_vec(wrapped),
        24 => aes_kw::KekAes192::new(GenericArray::from_slice(kek)).unwrap_vec(wrapped),
        32 => aes_kw::KekAes256::new(GenericArray::from_slice(kek)).unwrap_vec(wrapped),
        _ => return Err(Error::Crypto("bad key-encryption key length".into())),
    };
    plain.map(Zeroizing::new).map_err(failed)
}

fn gcm_seal(
    cipher: ContentCipher,
    key: &[u8],
    nonce: &[u8; 12],
    plaintext: &[u8],
) -> Result<Vec<u8>> {
    let nonce = aes_gcm::Nonce::from_slice(nonce);
    let failed = |_| Error::Crypto("AES-GCM encryption failed".into());
    match cipher {
        ContentCipher::Aes128Gcm => aes_gcm::Aes128Gcm::new_from_slice(key)
            .map_err(|_| Error::Crypto("bad key length".into()))?
            .encrypt(nonce, plaintext)
            .map_err(failed),
        _ => aes_gcm::Aes256Gcm::new_from_slice(key)
            .map_err(|_| Error::Crypto("bad key length".into()))?
            .encrypt(nonce, plaintext)
            .map_err(failed),
    }
}

/// A key that may decrypt: an own certificate and its private key
/// (`None` while the key is locked).
#[derive(Debug, Clone, Copy)]
pub struct DecryptionKey<'a> {
    /// The own certificate.
    pub cert: &'a Cert,
    /// Its private key, `None` when locked.
    pub key: Option<&'a dyn PrivateKeyOps>,
}

/// What an encrypted part says about itself.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Envelope {
    /// `AuthEnvelopedData` (AES-GCM) rather than `EnvelopedData`.
    pub authenticated: bool,
    /// Content cipher, e.g. `AES-256-GCM`.
    pub algorithm: String,
    /// Recipient identifiers as the message names them:
    /// `<issuer DN>/<serial hex>` or the subject key identifier in hex.
    pub recipients: Vec<String>,
}

/// Outcome of [`decrypt`].
#[derive(Debug)]
pub enum Decryption {
    /// Decrypted with the certificate of this fingerprint.
    Decrypted {
        /// The plaintext (a MIME entity).
        plaintext: Zeroizing<Vec<u8>>,
        /// Fingerprint of the own certificate that matched.
        fingerprint: String,
    },
    /// None of the keys is a recipient.
    NoKey,
    /// A recipient key exists but is locked.
    Locked {
        /// Fingerprint of the locked certificate.
        fingerprint: String,
    },
}

struct Parsed {
    envelope: Envelope,
    recipients: Vec<RecipientInfo>,
    content: EncryptedContentInfo,
    mac: Option<Vec<u8>>,
    aad: Vec<u8>,
}

fn parse(input: &[u8]) -> Result<Parsed> {
    let der = ber::normalize(input)?;
    let info = ContentInfo::from_der(&der)?;
    let (recipients, content, mac, aad, authenticated) = match info.content_type {
        oids::ID_ENVELOPED_DATA => {
            let data: EnvelopedData = info.content.decode_as()?;
            (
                data.recip_infos,
                data.encrypted_content,
                None,
                Vec::new(),
                false,
            )
        }
        oids::ID_AUTH_ENVELOPED_DATA => {
            let data: AuthEnvelopedData = info.content.decode_as()?;
            let aad = match &data.auth_attrs {
                Some(attrs) => attrs.to_der()?,
                None => Vec::new(),
            };
            (
                data.recip_infos,
                data.auth_encrypted_content,
                Some(data.mac.into_bytes()),
                aad,
                true,
            )
        }
        other => {
            return Err(Error::malformed(format!(
                "not an encrypted CMS type: {other}"
            )));
        }
    };
    let recipients: Vec<RecipientInfo> = recipients.0.into_vec();
    let mut names = Vec::new();
    for ri in &recipients {
        match ri {
            RecipientInfo::Ktri(k) => names.push(rid_name(&k.rid)),
            RecipientInfo::Kari(k) => {
                for rek in &k.recipient_enc_keys {
                    names.push(match &rek.rid {
                        KeyAgreeRecipientIdentifier::IssuerAndSerialNumber(id) => {
                            format!("{}/{}", id.issuer, hex_upper(id.serial_number.as_bytes()))
                        }
                        KeyAgreeRecipientIdentifier::RKeyId(k) => {
                            hex_upper(k.subject_key_identifier.0.as_bytes())
                        }
                    });
                }
            }
            _ => {}
        }
    }
    let algorithm = cipher_name(&content.content_enc_alg.oid);
    Ok(Parsed {
        envelope: Envelope {
            authenticated,
            algorithm,
            recipients: names,
        },
        recipients,
        content,
        mac,
        aad,
    })
}

fn rid_name(rid: &RecipientIdentifier) -> String {
    match rid {
        RecipientIdentifier::IssuerAndSerialNumber(id) => {
            format!("{}/{}", id.issuer, hex_upper(id.serial_number.as_bytes()))
        }
        RecipientIdentifier::SubjectKeyIdentifier(ski) => hex_upper(ski.0.as_bytes()),
    }
}

fn cipher_name(oid: &const_oid::ObjectIdentifier) -> String {
    match *oid {
        oids::AES128_CBC => "AES-128-CBC".into(),
        oids::AES192_CBC => "AES-192-CBC".into(),
        oids::AES256_CBC => "AES-256-CBC".into(),
        oids::AES128_GCM => "AES-128-GCM".into(),
        oids::AES192_GCM => "AES-192-GCM".into(),
        oids::AES256_GCM => "AES-256-GCM".into(),
        oids::DES_EDE3_CBC => "3DES".into(),
        oids::RC2_CBC => "RC2".into(),
        other => other.to_string(),
    }
}

/// Reads the envelope without decrypting (recipients, algorithm).
pub fn inspect(input: &[u8]) -> Result<Envelope> {
    Ok(parse(input)?.envelope)
}

/// Decrypts with the first matching key. `Err` means the data is broken,
/// tampered with, or uses an unsupported algorithm, or the matching key
/// failed; `NoKey` / `Locked` are normal outcomes.
pub fn decrypt(input: &[u8], keys: &[DecryptionKey<'_>]) -> Result<(Envelope, Decryption)> {
    let parsed = parse(input)?;
    let mut locked = None;
    let mut last_error = None;
    for ri in &parsed.recipients {
        for key in keys {
            let Some(target) = recipient_target(ri, key.cert) else {
                continue;
            };
            let Some(private) = key.key else {
                locked.get_or_insert_with(|| key.cert.fingerprint().to_owned());
                continue;
            };
            let result =
                recover_cek(target, private).and_then(|cek| decrypt_content(&parsed, &cek));
            match result {
                Ok(plaintext) => {
                    return Ok((
                        parsed.envelope,
                        Decryption::Decrypted {
                            plaintext,
                            fingerprint: key.cert.fingerprint().to_owned(),
                        },
                    ));
                }
                Err(err) => last_error = Some(err),
            }
        }
    }
    if let Some(err) = last_error {
        return Err(err);
    }
    let outcome = match locked {
        Some(fingerprint) => Decryption::Locked { fingerprint },
        None => Decryption::NoKey,
    };
    Ok((parsed.envelope, outcome))
}

/// Which encrypted key of a recipient info belongs to `cert`.
#[derive(Clone, Copy)]
enum Target<'r> {
    Ktri(&'r KeyTransRecipientInfo),
    Kari(&'r KeyAgreeRecipientInfo, &'r RecipientEncryptedKey),
}

fn recipient_target<'r>(ri: &'r RecipientInfo, cert: &Cert) -> Option<Target<'r>> {
    match ri {
        RecipientInfo::Ktri(k) => {
            let matches = match &k.rid {
                RecipientIdentifier::IssuerAndSerialNumber(id) => cert.matches_issuer_serial(id),
                RecipientIdentifier::SubjectKeyIdentifier(ski) => {
                    cert.matches_key_id(ski.0.as_bytes())
                }
            };
            matches.then_some(Target::Ktri(k))
        }
        RecipientInfo::Kari(k) => k
            .recipient_enc_keys
            .iter()
            .find(|rek| match &rek.rid {
                KeyAgreeRecipientIdentifier::IssuerAndSerialNumber(id) => {
                    cert.matches_issuer_serial(id)
                }
                KeyAgreeRecipientIdentifier::RKeyId(r) => {
                    cert.matches_key_id(r.subject_key_identifier.0.as_bytes())
                }
            })
            .map(|rek| Target::Kari(k, rek)),
        _ => None,
    }
}

fn recover_cek(target: Target<'_>, key: &dyn PrivateKeyOps) -> Result<Zeroizing<Vec<u8>>> {
    match target {
        Target::Ktri(k) => {
            let padding = RsaPadding::resolve(&k.key_enc_alg)?;
            key.decrypt_rsa(padding, k.enc_key.as_bytes())
        }
        Target::Kari(k, rek) => {
            let hash = match k.key_enc_alg.oid {
                oids::ECDH_STD_SHA1_KDF => HashAlg::Sha1,
                oids::ECDH_STD_SHA256_KDF => HashAlg::Sha256,
                oids::ECDH_STD_SHA384_KDF => HashAlg::Sha384,
                oids::ECDH_STD_SHA512_KDF => HashAlg::Sha512,
                other => return Err(Error::Unsupported(format!("key agreement {other}"))),
            };
            let wrap: AlgorithmIdentifierOwned = k
                .key_enc_alg
                .parameters
                .as_ref()
                .ok_or_else(|| Error::malformed("key agreement without a wrap algorithm"))?
                .decode_as()?;
            let OriginatorIdentifierOrKey::OriginatorKey(originator) = &k.originator else {
                return Err(Error::Unsupported("static-static ECDH".into()));
            };
            let point = originator
                .public_key
                .as_bytes()
                .ok_or_else(|| Error::malformed("originator key is not octet-aligned"))?;
            let z = key.ecdh(point)?;
            let kek = kek_from_shared(hash, &z, &wrap, k.ukm.as_ref())?;
            aes_unwrap(&kek, rek.enc_key.as_bytes())
        }
    }
}

fn decrypt_content(parsed: &Parsed, cek: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    let alg = &parsed.content.content_enc_alg;
    let ciphertext = parsed
        .content
        .encrypted_content
        .as_ref()
        .ok_or_else(|| Error::Unsupported("detached encrypted content".into()))?
        .as_bytes();
    let bad_key = || Error::Crypto("content key has the wrong length".into());
    let failed = || Error::Crypto("content decryption failed (wrong key or damaged data)".into());
    match (alg.oid, &parsed.mac) {
        (oids::AES128_CBC | oids::AES192_CBC | oids::AES256_CBC, None) => {
            let iv: OctetString = alg
                .parameters
                .as_ref()
                .ok_or_else(|| Error::malformed("CBC without an IV"))?
                .decode_as()?;
            let iv = iv.as_bytes();
            let plain = match alg.oid {
                oids::AES128_CBC => cbc::Decryptor::<aes::Aes128>::new_from_slices(cek, iv)
                    .map_err(|_| bad_key())?
                    .decrypt_padded_vec_mut::<Pkcs7>(ciphertext),
                oids::AES192_CBC => cbc::Decryptor::<aes::Aes192>::new_from_slices(cek, iv)
                    .map_err(|_| bad_key())?
                    .decrypt_padded_vec_mut::<Pkcs7>(ciphertext),
                _ => cbc::Decryptor::<aes::Aes256>::new_from_slices(cek, iv)
                    .map_err(|_| bad_key())?
                    .decrypt_padded_vec_mut::<Pkcs7>(ciphertext),
            };
            plain.map(Zeroizing::new).map_err(|_| failed())
        }
        (oids::AES128_GCM | oids::AES256_GCM, Some(mac)) => {
            let params: GcmParameters = alg
                .parameters
                .as_ref()
                .ok_or_else(|| Error::malformed("GCM without parameters"))?
                .decode_as()?;
            if params.nonce.as_bytes().len() != 12 || params.icv_len != 16 || mac.len() != 16 {
                return Err(Error::Unsupported("GCM nonce / tag length".into()));
            }
            let nonce = aes_gcm::Nonce::from_slice(params.nonce.as_bytes());
            let mut sealed = Vec::with_capacity(ciphertext.len() + 16);
            sealed.extend_from_slice(ciphertext);
            sealed.extend_from_slice(mac);
            let payload = Payload {
                msg: &sealed,
                aad: &parsed.aad,
            };
            let plain = if alg.oid == oids::AES128_GCM {
                aes_gcm::Aes128Gcm::new_from_slice(cek)
                    .map_err(|_| bad_key())?
                    .decrypt(nonce, payload)
            } else {
                aes_gcm::Aes256Gcm::new_from_slice(cek)
                    .map_err(|_| bad_key())?
                    .decrypt(nonce, payload)
            };
            plain.map(Zeroizing::new).map_err(|_| failed())
        }
        (oid, _) => Err(Error::Unsupported(format!(
            "content encryption {}",
            cipher_name(&oid)
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::SoftwareKey;
    use crate::testutil::{fixture_cert, fixture_key};

    fn key<'a>(cert: &'a Cert, key: &'a SoftwareKey) -> DecryptionKey<'a> {
        DecryptionKey {
            cert,
            key: Some(key),
        }
    }

    fn plaintext(d: Decryption) -> Vec<u8> {
        match d {
            Decryption::Decrypted { plaintext, .. } => plaintext.to_vec(),
            other => panic!("not decrypted: {other:?}"),
        }
    }

    #[test]
    fn every_cipher_round_trips_to_rsa_and_ec_recipients() {
        let alice = fixture_cert("alice.crt");
        let alice_key = fixture_key("alice.key");
        let bob = fixture_cert("bob.crt");
        let bob_key = fixture_key("bob.key");
        for cipher in [
            ContentCipher::Aes128Gcm,
            ContentCipher::Aes256Gcm,
            ContentCipher::Aes128Cbc,
            ContentCipher::Aes256Cbc,
        ] {
            let der = encrypt(b"secret entity", &[alice.clone(), bob.clone()], cipher).unwrap();
            let envelope = inspect(&der).unwrap();
            assert_eq!(envelope.authenticated, cipher.is_authenticated());
            assert_eq!(envelope.recipients.len(), 2);
            let (env, d) = decrypt(&der, &[key(&alice, &alice_key)]).unwrap();
            assert_eq!(plaintext(d), b"secret entity");
            let (_, d) = decrypt(&der, &[key(&bob, &bob_key)]).unwrap();
            assert_eq!(plaintext(d), b"secret entity");
            let expected = match cipher {
                ContentCipher::Aes128Gcm => "AES-128-GCM",
                ContentCipher::Aes256Gcm => "AES-256-GCM",
                ContentCipher::Aes128Cbc => "AES-128-CBC",
                ContentCipher::Aes256Cbc => "AES-256-CBC",
            };
            assert_eq!(env.algorithm, expected);
        }
    }

    #[test]
    fn wrong_recipient_has_no_key() {
        let alice = fixture_cert("alice.crt");
        let erin = fixture_cert("erin.crt");
        let erin_key = fixture_key("erin.key");
        let der = encrypt(b"x", &[alice], ContentCipher::default()).unwrap();
        let (_, d) = decrypt(&der, &[key(&erin, &erin_key)]).unwrap();
        assert!(matches!(d, Decryption::NoKey));
        let (_, d) = decrypt(&der, &[]).unwrap();
        assert!(matches!(d, Decryption::NoKey));
    }

    #[test]
    fn a_locked_matching_key_is_reported() {
        let alice = fixture_cert("alice.crt");
        let der = encrypt(b"x", std::slice::from_ref(&alice), ContentCipher::default()).unwrap();
        let (_, d) = decrypt(
            &der,
            &[DecryptionKey {
                cert: &alice,
                key: None,
            }],
        )
        .unwrap();
        let Decryption::Locked { fingerprint } = d else {
            panic!("locked")
        };
        assert_eq!(fingerprint, alice.fingerprint());
    }

    #[test]
    fn a_key_that_does_not_match_its_certificate_fails() {
        // The certificate names alice, but the key is erin's.
        let alice = fixture_cert("alice.crt");
        let erin_key = fixture_key("erin.key");
        let der = encrypt(b"x", std::slice::from_ref(&alice), ContentCipher::Aes256Gcm).unwrap();
        assert!(matches!(
            decrypt(&der, &[key(&alice, &erin_key)]),
            Err(Error::Crypto(_))
        ));
    }

    #[test]
    fn tampering_is_detected_by_gcm() {
        let alice = fixture_cert("alice.crt");
        let alice_key = fixture_key("alice.key");
        let der = encrypt(
            &[b'a'; 200],
            std::slice::from_ref(&alice),
            ContentCipher::Aes256Gcm,
        )
        .unwrap();
        // Flip a byte of the ciphertext (near the end, before the tag).
        let mut tampered = der.clone();
        let pos = tampered.len() - 30;
        tampered[pos] ^= 0x80;
        assert!(matches!(
            decrypt(&tampered, &[key(&alice, &alice_key)]),
            Err(Error::Crypto(_))
        ));
        // Flip a byte of the tag (the last element).
        let mut tampered = der;
        let last = tampered.len() - 1;
        tampered[last] ^= 0x01;
        assert!(decrypt(&tampered, &[key(&alice, &alice_key)]).is_err());
    }

    #[test]
    fn cbc_padding_damage_is_an_error_not_garbage() {
        let alice = fixture_cert("alice.crt");
        let alice_key = fixture_key("alice.key");
        let der = encrypt(
            b"short",
            std::slice::from_ref(&alice),
            ContentCipher::Aes128Cbc,
        )
        .unwrap();
        let mut tampered = der.clone();
        let last = tampered.len() - 1;
        tampered[last] ^= 0xff;
        assert!(decrypt(&tampered, &[key(&alice, &alice_key)]).is_err());
    }

    #[test]
    fn certificates_that_cannot_encrypt_are_refused() {
        let dave = fixture_cert("dave.crt"); // serverAuth only
        assert!(matches!(
            encrypt(b"x", &[dave], ContentCipher::default()),
            Err(Error::NoKey(_))
        ));
        assert!(matches!(
            encrypt(b"x", &[], ContentCipher::default()),
            Err(Error::NoKey(_))
        ));
        let ca = fixture_cert("root-ca.crt");
        assert!(encrypt(b"x", &[ca], ContentCipher::default()).is_err());
    }

    #[test]
    fn kdf_matches_a_known_answer() {
        // X9.63 KDF with SHA-256 (test vector from NIST CAVS
        // "ansx963_2001.rsp", SHA-256, shared secret length 192, no
        // shared info, key data length 128).
        let z = hex("96c05619d56c328ab95fe84b18264b08725b85e33fd34f08");
        let out = x963_kdf(HashAlg::Sha256, &z, &[], 16);
        assert_eq!(
            out.as_slice(),
            hex("443024c3dae66b95e6f5670601558f71").as_slice()
        );
    }

    #[test]
    fn aes_key_wrap_matches_rfc_3394() {
        // RFC 3394 section 4.1: 128-bit KEK, 128-bit key data.
        let kek = hex("000102030405060708090A0B0C0D0E0F");
        let data = hex("00112233445566778899AABBCCDDEEFF");
        let wrapped = aes_wrap(&kek, &data).unwrap();
        assert_eq!(
            wrapped,
            hex("1FA68B0A8112B447AEF34BD8FB5A7B829D3E862371D2CFE5")
        );
        assert_eq!(
            aes_unwrap(&kek, &wrapped).unwrap().as_slice(),
            data.as_slice()
        );
        let mut bad = wrapped;
        bad[0] ^= 1;
        assert!(aes_unwrap(&kek, &bad).is_err());
        assert!(aes_wrap(&[0; 5], &data).is_err());
    }

    fn hex(text: &str) -> Vec<u8> {
        (0..text.len())
            .step_by(2)
            .map(|i| u8::from_str_radix(&text[i..i + 2], 16).unwrap())
            .collect()
    }

    #[test]
    fn not_an_envelope() {
        assert!(inspect(b"junk").is_err());
        let signed = crate::signed::sign(
            b"x",
            &crate::signed::Signer {
                key: &fixture_key("alice.key"),
                cert: &fixture_cert("alice.crt"),
                chain: &[],
            },
            false,
            0,
        )
        .unwrap();
        assert!(inspect(&signed).is_err());
    }
}
