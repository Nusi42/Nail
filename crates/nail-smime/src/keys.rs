//! Public and private keys: RSA, ECDSA / ECDH on P-256 and P-384.
//!
//! Private-key operations go through [`PrivateKeyOps`], so a key held in
//! software ([`SoftwareKey`], from PKCS#12) and a key that never leaves the
//! Windows certificate store (`windows::WindowsKey`) are used the same
//! way by signing and decryption.

use std::fmt;

use const_oid::ObjectIdentifier;
use der::{Decode, Encode};
use rand::rngs::OsRng;
use rsa::pkcs8::{DecodePrivateKey, DecodePublicKey};
use rsa::{Oaep, Pkcs1v15Encrypt, Pkcs1v15Sign, Pss, RsaPrivateKey, RsaPublicKey};
use sha2_legacy::{Digest, Sha256, Sha384, Sha512};
use spki::{AlgorithmIdentifierOwned, SubjectPublicKeyInfoOwned};
use zeroize::Zeroizing;

use crate::error::{Error, Result};
use crate::oids;

/// A digest algorithm.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum HashAlg {
    /// SHA-1: never accepted for signatures (RFC 8551 section 2.1); only
    /// for RSA-OAEP's default parameters and certificate thumbprints.
    Sha1,
    /// SHA-256 (the default).
    Sha256,
    /// SHA-384 (used with P-384 keys).
    Sha384,
    /// SHA-512.
    Sha512,
}

impl HashAlg {
    /// From a digest algorithm OID.
    pub fn from_oid(oid: &ObjectIdentifier) -> Option<Self> {
        Some(match *oid {
            oids::ID_SHA1 => Self::Sha1,
            oids::ID_SHA256 => Self::Sha256,
            oids::ID_SHA384 => Self::Sha384,
            oids::ID_SHA512 => Self::Sha512,
            _ => return None,
        })
    }

    /// The digest algorithm OID.
    pub fn oid(self) -> ObjectIdentifier {
        match self {
            Self::Sha1 => oids::ID_SHA1,
            Self::Sha256 => oids::ID_SHA256,
            Self::Sha384 => oids::ID_SHA384,
            Self::Sha512 => oids::ID_SHA512,
        }
    }

    /// The algorithm identifier (parameters absent, RFC 5754).
    pub fn algorithm_identifier(self) -> AlgorithmIdentifierOwned {
        AlgorithmIdentifierOwned {
            oid: self.oid(),
            parameters: None,
        }
    }

    /// Hashes `data`.
    pub fn digest(self, data: &[u8]) -> Vec<u8> {
        match self {
            Self::Sha1 => sha1::Sha1::digest(data).to_vec(),
            Self::Sha256 => Sha256::digest(data).to_vec(),
            Self::Sha384 => Sha384::digest(data).to_vec(),
            Self::Sha512 => Sha512::digest(data).to_vec(),
        }
    }

    /// The `micalg` parameter of `multipart/signed` (RFC 8551 section 3.5.3).
    pub fn micalg(self) -> &'static str {
        match self {
            Self::Sha1 => "sha-1",
            Self::Sha256 => "sha-256",
            Self::Sha384 => "sha-384",
            Self::Sha512 => "sha-512",
        }
    }

    fn pkcs1v15(self) -> Pkcs1v15Sign {
        match self {
            Self::Sha1 => Pkcs1v15Sign::new::<sha1::Sha1>(),
            Self::Sha256 => Pkcs1v15Sign::new::<Sha256>(),
            Self::Sha384 => Pkcs1v15Sign::new::<Sha384>(),
            Self::Sha512 => Pkcs1v15Sign::new::<Sha512>(),
        }
    }
}

/// How a signature is computed, resolved from the algorithm identifiers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SignScheme {
    /// RSASSA-PKCS1-v1_5.
    RsaPkcs1(HashAlg),
    /// RSASSA-PSS with MGF1 over the same hash.
    RsaPss {
        /// The hash (message and MGF1).
        hash: HashAlg,
        /// Salt length in bytes.
        salt_len: usize,
    },
    /// ECDSA (DER-encoded `r`, `s`).
    Ecdsa(HashAlg),
}

impl SignScheme {
    /// The hash of the scheme.
    pub fn hash(self) -> HashAlg {
        match self {
            Self::RsaPkcs1(h) | Self::Ecdsa(h) | Self::RsaPss { hash: h, .. } => h,
        }
    }

    /// Resolves a signature algorithm identifier. `digest` is the separate
    /// digest algorithm of a CMS `SignerInfo` (needed when the signature
    /// algorithm is just `rsaEncryption` / `id-ecPublicKey`). SHA-1 is
    /// refused.
    pub fn resolve(algorithm: &AlgorithmIdentifierOwned, digest: Option<HashAlg>) -> Result<Self> {
        let need_digest =
            || digest.ok_or_else(|| Error::Unsupported(format!("digest for {}", algorithm.oid)));
        let scheme = match algorithm.oid {
            oids::RSA_ENCRYPTION => Self::RsaPkcs1(need_digest()?),
            oids::SHA1_WITH_RSA => Self::RsaPkcs1(HashAlg::Sha1),
            oids::SHA256_WITH_RSA => Self::RsaPkcs1(HashAlg::Sha256),
            oids::SHA384_WITH_RSA => Self::RsaPkcs1(HashAlg::Sha384),
            oids::SHA512_WITH_RSA => Self::RsaPkcs1(HashAlg::Sha512),
            oids::EC_PUBLIC_KEY => Self::Ecdsa(need_digest()?),
            oids::ECDSA_WITH_SHA1 => Self::Ecdsa(HashAlg::Sha1),
            oids::ECDSA_WITH_SHA256 => Self::Ecdsa(HashAlg::Sha256),
            oids::ECDSA_WITH_SHA384 => Self::Ecdsa(HashAlg::Sha384),
            oids::ECDSA_WITH_SHA512 => Self::Ecdsa(HashAlg::Sha512),
            oids::RSASSA_PSS => resolve_pss(algorithm)?,
            other => {
                return Err(Error::Unsupported(format!("signature algorithm {other}")));
            }
        };
        if scheme.hash() == HashAlg::Sha1 {
            return Err(Error::Unsupported("SHA-1 signatures".into()));
        }
        Ok(scheme)
    }

    /// The `SignerInfo.signatureAlgorithm` Nail writes: `rsaEncryption`
    /// (what every S/MIME client accepts, RFC 3370) or `ecdsa-with-SHA*`.
    pub fn cms_algorithm(self) -> Result<AlgorithmIdentifierOwned> {
        let oid = match self {
            Self::RsaPkcs1(_) => {
                return Ok(AlgorithmIdentifierOwned {
                    oid: oids::RSA_ENCRYPTION,
                    parameters: Some(der::asn1::Null.into()),
                });
            }
            Self::Ecdsa(HashAlg::Sha256) => oids::ECDSA_WITH_SHA256,
            Self::Ecdsa(HashAlg::Sha384) => oids::ECDSA_WITH_SHA384,
            Self::Ecdsa(HashAlg::Sha512) => oids::ECDSA_WITH_SHA512,
            other => return Err(Error::Unsupported(format!("signing with {other:?}"))),
        };
        Ok(AlgorithmIdentifierOwned {
            oid,
            parameters: None,
        })
    }
}

fn resolve_pss(algorithm: &AlgorithmIdentifierOwned) -> Result<SignScheme> {
    let params = algorithm
        .parameters
        .as_ref()
        .ok_or_else(|| Error::malformed("RSASSA-PSS without parameters"))?;
    let bytes = params.to_der()?;
    let params = rsa::pkcs1::RsaPssParams::from_der(&bytes)?;
    let hash = HashAlg::from_oid(&params.hash.oid)
        .ok_or_else(|| Error::Unsupported(format!("PSS hash {}", params.hash.oid)))?;
    let mgf_hash = params
        .mask_gen
        .parameters
        .as_ref()
        .and_then(|p| HashAlg::from_oid(&p.oid));
    if params.mask_gen.oid != oids::ID_MGF1 || mgf_hash != Some(hash) {
        return Err(Error::Unsupported(
            "PSS with another mask generation".into(),
        ));
    }
    Ok(SignScheme::RsaPss {
        hash,
        salt_len: usize::from(params.salt_len),
    })
}

/// RSA encryption padding of a key-transport recipient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RsaPadding {
    /// RSAES-PKCS1-v1_5 (what nearly every client sends).
    Pkcs1v15,
    /// RSAES-OAEP with this hash (label empty, MGF1 over the same hash).
    Oaep(HashAlg),
}

impl RsaPadding {
    /// From a `keyEncryptionAlgorithm`.
    pub fn resolve(algorithm: &AlgorithmIdentifierOwned) -> Result<Self> {
        match algorithm.oid {
            oids::RSA_ENCRYPTION => Ok(Self::Pkcs1v15),
            oids::RSAES_OAEP => {
                let hash = match &algorithm.parameters {
                    None => HashAlg::Sha1,
                    Some(p) => {
                        let bytes = p.to_der()?;
                        let params = rsa::pkcs1::RsaOaepParams::from_der(&bytes)?;
                        let hash = HashAlg::from_oid(&params.hash.oid).ok_or_else(|| {
                            Error::Unsupported(format!("OAEP hash {}", params.hash.oid))
                        })?;
                        let mgf = params
                            .mask_gen
                            .parameters
                            .as_ref()
                            .and_then(|p| HashAlg::from_oid(&p.oid));
                        if mgf != Some(hash) {
                            return Err(Error::Unsupported("OAEP with another MGF1 hash".into()));
                        }
                        if !params
                            .p_source
                            .parameters
                            .as_ref()
                            .is_none_or(|p| p.value().is_empty())
                        {
                            return Err(Error::Unsupported("OAEP with a label".into()));
                        }
                        hash
                    }
                };
                Ok(Self::Oaep(hash))
            }
            other => Err(Error::Unsupported(format!("key transport {other}"))),
        }
    }
}

/// A public key from a certificate.
#[derive(Clone, PartialEq, Eq)]
pub enum PublicKey {
    /// RSA.
    Rsa(RsaPublicKey),
    /// NIST P-256.
    P256(p256::PublicKey),
    /// NIST P-384.
    P384(p384::PublicKey),
}

impl fmt::Debug for PublicKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.algorithm_name())
    }
}

impl PublicKey {
    /// From a `SubjectPublicKeyInfo`.
    pub fn from_spki(spki: &SubjectPublicKeyInfoOwned) -> Result<Self> {
        let der = spki.to_der()?;
        match spki.algorithm.oid {
            oids::RSA_ENCRYPTION => Ok(Self::Rsa(
                RsaPublicKey::from_public_key_der(&der).map_err(Error::malformed)?,
            )),
            oids::EC_PUBLIC_KEY => {
                let curve = spki
                    .algorithm
                    .parameters
                    .as_ref()
                    .and_then(|p| p.decode_as::<ObjectIdentifier>().ok());
                by_curve(
                    curve,
                    || {
                        p256::PublicKey::from_public_key_der(&der)
                            .map(Self::P256)
                            .map_err(Error::malformed)
                    },
                    || {
                        p384::PublicKey::from_public_key_der(&der)
                            .map(Self::P384)
                            .map_err(Error::malformed)
                    },
                )
            }
            oids::ED25519 => Err(Error::Unsupported("Ed25519 certificates".into())),
            other => Err(Error::Unsupported(format!("public key algorithm {other}"))),
        }
    }

    /// Display name, e.g. `RSA 2048`, `ECDSA P-256`.
    pub fn algorithm_name(&self) -> String {
        match self {
            Self::Rsa(key) => format!("RSA {}", rsa::traits::PublicKeyParts::size(key) * 8),
            Self::P256(_) => "ECDSA P-256".to_owned(),
            Self::P384(_) => "ECDSA P-384".to_owned(),
        }
    }

    /// The hash Nail signs with for this key: SHA-384 for P-384,
    /// SHA-256 otherwise.
    pub fn preferred_hash(&self) -> HashAlg {
        match self {
            Self::P384(_) => HashAlg::Sha384,
            _ => HashAlg::Sha256,
        }
    }

    /// The signature scheme Nail uses with this key.
    pub fn default_scheme(&self) -> SignScheme {
        match self {
            Self::Rsa(_) => SignScheme::RsaPkcs1(self.preferred_hash()),
            _ => SignScheme::Ecdsa(self.preferred_hash()),
        }
    }

    /// Uncompressed SEC1 point of an EC key.
    pub fn ec_point(&self) -> Option<Vec<u8>> {
        use p256::elliptic_curve::sec1::ToEncodedPoint;
        match self {
            Self::Rsa(_) => None,
            Self::P256(key) => Some(key.to_encoded_point(false).as_bytes().to_vec()),
            Self::P384(key) => Some(key.to_encoded_point(false).as_bytes().to_vec()),
        }
    }

    /// Checks `signature` over `message` (the message is hashed here).
    pub fn verify(&self, scheme: SignScheme, message: &[u8], signature: &[u8]) -> Result<()> {
        let digest = scheme.hash().digest(message);
        self.verify_digest(scheme, &digest, signature)
    }

    /// Checks `signature` over an already computed digest.
    pub fn verify_digest(&self, scheme: SignScheme, digest: &[u8], signature: &[u8]) -> Result<()> {
        use p256::ecdsa::signature::hazmat::PrehashVerifier;
        fn bad<E>(_: E) -> Error {
            Error::Crypto("signature does not verify".into())
        }
        match (self, scheme) {
            (Self::Rsa(key), SignScheme::RsaPkcs1(hash)) => {
                key.verify(hash.pkcs1v15(), digest, signature).map_err(bad)
            }
            (Self::Rsa(key), SignScheme::RsaPss { hash, salt_len }) => {
                let pss = match hash {
                    HashAlg::Sha1 => Pss::new_with_salt::<sha1::Sha1>(salt_len),
                    HashAlg::Sha256 => Pss::new_with_salt::<Sha256>(salt_len),
                    HashAlg::Sha384 => Pss::new_with_salt::<Sha384>(salt_len),
                    HashAlg::Sha512 => Pss::new_with_salt::<Sha512>(salt_len),
                };
                key.verify(pss, digest, signature).map_err(bad)
            }
            (Self::P256(key), SignScheme::Ecdsa(_)) => {
                let sig = p256::ecdsa::DerSignature::from_bytes(signature).map_err(bad)?;
                let sig = p256::ecdsa::Signature::try_from(sig).map_err(bad)?;
                p256::ecdsa::VerifyingKey::from(key)
                    .verify_prehash(digest, &sig)
                    .map_err(bad)
            }
            (Self::P384(key), SignScheme::Ecdsa(_)) => {
                let sig = p384::ecdsa::DerSignature::from_bytes(signature).map_err(bad)?;
                let sig = p384::ecdsa::Signature::try_from(sig).map_err(bad)?;
                p384::ecdsa::VerifyingKey::from(key)
                    .verify_prehash(digest, &sig)
                    .map_err(bad)
            }
            _ => Err(Error::Crypto(
                "signature algorithm does not match the key".into(),
            )),
        }
    }
}

/// Operations with a private key, wherever it lives.
pub trait PrivateKeyOps: Send + Sync + fmt::Debug {
    /// Signs a digest; RSA returns the raw signature, ECDSA the DER form.
    fn sign_digest(&self, scheme: SignScheme, digest: &[u8]) -> Result<Vec<u8>>;
    /// RSA key transport: decrypts a content-encryption key.
    fn decrypt_rsa(&self, padding: RsaPadding, ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>>;
    /// ECDH: the raw shared secret (the x coordinate) with a peer's
    /// uncompressed SEC1 point.
    fn ecdh(&self, peer_point: &[u8]) -> Result<Zeroizing<Vec<u8>>>;
}

/// A private key held in memory (from PKCS#12 or the keyring). Zeroized on
/// drop by the RustCrypto types.
pub enum SoftwareKey {
    /// RSA.
    Rsa(Box<RsaPrivateKey>),
    /// P-256.
    P256(p256::SecretKey),
    /// P-384.
    P384(p384::SecretKey),
}

impl fmt::Debug for SoftwareKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SoftwareKey({})", self.public_key().algorithm_name())
    }
}

impl SoftwareKey {
    /// From an unencrypted PKCS#8 `PrivateKeyInfo`.
    pub fn from_pkcs8_der(der: &[u8]) -> Result<Self> {
        let info = rsa::pkcs8::PrivateKeyInfo::try_from(der).map_err(Error::malformed)?;
        match info.algorithm.oid {
            oids::RSA_ENCRYPTION => Ok(Self::Rsa(Box::new(
                RsaPrivateKey::from_pkcs8_der(der).map_err(Error::malformed)?,
            ))),
            oids::EC_PUBLIC_KEY => {
                let curve = info
                    .algorithm
                    .parameters
                    .and_then(|p| p.decode_as::<ObjectIdentifier>().ok());
                by_curve(
                    curve,
                    || {
                        p256::SecretKey::from_pkcs8_der(der)
                            .map(Self::P256)
                            .map_err(Error::malformed)
                    },
                    || {
                        p384::SecretKey::from_pkcs8_der(der)
                            .map(Self::P384)
                            .map_err(Error::malformed)
                    },
                )
            }
            other => Err(Error::Unsupported(format!("private key algorithm {other}"))),
        }
    }

    /// The matching public key.
    pub fn public_key(&self) -> PublicKey {
        match self {
            Self::Rsa(key) => PublicKey::Rsa(key.to_public_key()),
            Self::P256(key) => PublicKey::P256(key.public_key()),
            Self::P384(key) => PublicKey::P384(key.public_key()),
        }
    }
}

impl PrivateKeyOps for SoftwareKey {
    fn sign_digest(&self, scheme: SignScheme, digest: &[u8]) -> Result<Vec<u8>> {
        use p256::ecdsa::signature::hazmat::PrehashSigner;
        let failed = |e: &dyn fmt::Display| Error::Crypto(format!("signing failed: {e}"));
        match (self, scheme) {
            (Self::Rsa(key), SignScheme::RsaPkcs1(hash)) => key
                .sign_with_rng(&mut OsRng, hash.pkcs1v15(), digest)
                .map_err(|e| failed(&e)),
            (Self::P256(key), SignScheme::Ecdsa(_)) => {
                let signer = p256::ecdsa::SigningKey::from(key);
                let sig: p256::ecdsa::Signature =
                    signer.sign_prehash(digest).map_err(|e| failed(&e))?;
                Ok(sig.to_der().as_bytes().to_vec())
            }
            (Self::P384(key), SignScheme::Ecdsa(_)) => {
                let signer = p384::ecdsa::SigningKey::from(key);
                let sig: p384::ecdsa::Signature =
                    signer.sign_prehash(digest).map_err(|e| failed(&e))?;
                Ok(sig.to_der().as_bytes().to_vec())
            }
            (_, scheme) => Err(Error::Unsupported(format!(
                "signing with {scheme:?} and {self:?}"
            ))),
        }
    }

    fn decrypt_rsa(&self, padding: RsaPadding, ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let Self::Rsa(key) = self else {
            return Err(Error::Crypto("not an RSA key".into()));
        };
        let failed = |_| Error::Crypto("key transport decryption failed".into());
        let plain = match padding {
            RsaPadding::Pkcs1v15 => key
                .decrypt_blinded(&mut OsRng, Pkcs1v15Encrypt, ciphertext)
                .map_err(failed)?,
            RsaPadding::Oaep(hash) => {
                let oaep = match hash {
                    HashAlg::Sha1 => Oaep::new::<sha1::Sha1>(),
                    HashAlg::Sha256 => Oaep::new::<Sha256>(),
                    HashAlg::Sha384 => Oaep::new::<Sha384>(),
                    HashAlg::Sha512 => Oaep::new::<Sha512>(),
                };
                key.decrypt_blinded(&mut OsRng, oaep, ciphertext)
                    .map_err(failed)?
            }
        };
        Ok(Zeroizing::new(plain))
    }

    fn ecdh(&self, peer_point: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let bad_point = |_| Error::malformed("originator key is not a point on the curve");
        match self {
            Self::P256(key) => {
                let peer = p256::PublicKey::from_sec1_bytes(peer_point).map_err(bad_point)?;
                let shared = p256::ecdh::diffie_hellman(key.to_nonzero_scalar(), peer.as_affine());
                Ok(Zeroizing::new(shared.raw_secret_bytes().to_vec()))
            }
            Self::P384(key) => {
                let peer = p384::PublicKey::from_sec1_bytes(peer_point).map_err(bad_point)?;
                let shared = p384::ecdh::diffie_hellman(key.to_nonzero_scalar(), peer.as_affine());
                Ok(Zeroizing::new(shared.raw_secret_bytes().to_vec()))
            }
            Self::Rsa(_) => Err(Error::Crypto("not an EC key".into())),
        }
    }
}

/// The key of a named EC curve: `p256()` for P-256, `p384()` for P-384;
/// other curves are unsupported, a missing curve is malformed.
fn by_curve<T>(
    curve: Option<ObjectIdentifier>,
    p256: impl FnOnce() -> Result<T>,
    p384: impl FnOnce() -> Result<T>,
) -> Result<T> {
    match curve {
        Some(oids::SECP256R1) => p256(),
        Some(oids::SECP384R1) => p384(),
        Some(other) => Err(Error::Unsupported(format!("curve {other}"))),
        None => Err(Error::malformed("EC key without a named curve")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rsa_key() -> SoftwareKey {
        // 1024 bits keeps the test fast; never used for anything real.
        SoftwareKey::Rsa(Box::new(RsaPrivateKey::new(&mut OsRng, 1024).unwrap()))
    }

    #[test]
    fn hash_names_and_oids_round_trip() {
        for hash in [
            HashAlg::Sha1,
            HashAlg::Sha256,
            HashAlg::Sha384,
            HashAlg::Sha512,
        ] {
            assert_eq!(HashAlg::from_oid(&hash.oid()), Some(hash));
        }
        assert_eq!(HashAlg::Sha256.micalg(), "sha-256");
        assert_eq!(HashAlg::Sha384.digest(b"").len(), 48);
        assert_eq!(HashAlg::from_oid(&oids::RSA_ENCRYPTION), None);
    }

    #[test]
    fn sha1_signatures_are_refused() {
        let alg = AlgorithmIdentifierOwned {
            oid: oids::SHA1_WITH_RSA,
            parameters: None,
        };
        assert!(matches!(
            SignScheme::resolve(&alg, None),
            Err(Error::Unsupported(_))
        ));
        let alg = AlgorithmIdentifierOwned {
            oid: oids::RSA_ENCRYPTION,
            parameters: None,
        };
        assert!(SignScheme::resolve(&alg, Some(HashAlg::Sha1)).is_err());
        assert_eq!(
            SignScheme::resolve(&alg, Some(HashAlg::Sha256)).unwrap(),
            SignScheme::RsaPkcs1(HashAlg::Sha256)
        );
        // rsaEncryption without a digest cannot be resolved.
        assert!(SignScheme::resolve(&alg, None).is_err());
    }

    #[test]
    fn rsa_sign_verify_and_key_transport() {
        let key = rsa_key();
        let public = key.public_key();
        assert_eq!(public.algorithm_name(), "RSA 1024");
        let scheme = public.default_scheme();
        let digest = scheme.hash().digest(b"attributes");
        let sig = key.sign_digest(scheme, &digest).unwrap();
        public.verify(scheme, b"attributes", &sig).unwrap();
        assert!(public.verify(scheme, b"tampered", &sig).is_err());

        let PublicKey::Rsa(rsa_pub) = &public else {
            unreachable!()
        };
        let ct = rsa_pub
            .encrypt(&mut OsRng, Pkcs1v15Encrypt, b"content key")
            .unwrap();
        assert_eq!(
            &*key.decrypt_rsa(RsaPadding::Pkcs1v15, &ct).unwrap(),
            b"content key"
        );
        let ct = rsa_pub
            .encrypt(&mut OsRng, Oaep::new::<Sha256>(), b"content key")
            .unwrap();
        assert_eq!(
            &*key
                .decrypt_rsa(RsaPadding::Oaep(HashAlg::Sha256), &ct)
                .unwrap(),
            b"content key"
        );
        // Wrong padding fails instead of returning garbage.
        assert!(key.decrypt_rsa(RsaPadding::Pkcs1v15, &ct).is_err());
        assert!(key.ecdh(&[4u8; 65]).is_err());
    }

    #[test]
    fn ecdsa_and_ecdh_on_both_curves() {
        let a = SoftwareKey::P256(p256::SecretKey::random(&mut OsRng));
        let b = SoftwareKey::P256(p256::SecretKey::random(&mut OsRng));
        let scheme = a.public_key().default_scheme();
        assert_eq!(scheme, SignScheme::Ecdsa(HashAlg::Sha256));
        let sig = a
            .sign_digest(scheme, &scheme.hash().digest(b"data"))
            .unwrap();
        a.public_key().verify(scheme, b"data", &sig).unwrap();
        assert!(b.public_key().verify(scheme, b"data", &sig).is_err());
        let secret_of_a = a.ecdh(&b.public_key().ec_point().unwrap()).unwrap();
        let secret_of_b = b.ecdh(&a.public_key().ec_point().unwrap()).unwrap();
        assert_eq!(secret_of_a, secret_of_b);
        assert_eq!(secret_of_a.len(), 32);

        let c = SoftwareKey::P384(p384::SecretKey::random(&mut OsRng));
        let scheme = c.public_key().default_scheme();
        assert_eq!(scheme, SignScheme::Ecdsa(HashAlg::Sha384));
        let sig = c
            .sign_digest(scheme, &scheme.hash().digest(b"data"))
            .unwrap();
        c.public_key().verify(scheme, b"data", &sig).unwrap();
        assert_eq!(c.public_key().algorithm_name(), "ECDSA P-384");
        // A P-256 point is not a P-384 point.
        assert!(c.ecdh(&a.public_key().ec_point().unwrap()).is_err());
        assert!(c.decrypt_rsa(RsaPadding::Pkcs1v15, b"x").is_err());
    }

    #[test]
    fn mismatched_scheme_is_an_error() {
        let a = SoftwareKey::P256(p256::SecretKey::random(&mut OsRng));
        let scheme = SignScheme::RsaPkcs1(HashAlg::Sha256);
        assert!(a.sign_digest(scheme, &[0; 32]).is_err());
        assert!(a.public_key().verify(scheme, b"x", &[0; 64]).is_err());
    }

    #[test]
    fn pkcs8_rejects_garbage() {
        assert!(SoftwareKey::from_pkcs8_der(b"nope").is_err());
    }
}
