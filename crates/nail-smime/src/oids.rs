//! Object identifiers used by S/MIME (spelled out, so each one can be
//! checked against its RFC at a glance).

use const_oid::ObjectIdentifier as Oid;

// Content types (RFC 5652, RFC 5083).
/// id-data.
pub const ID_DATA: Oid = Oid::new_unwrap("1.2.840.113549.1.7.1");
/// id-signedData.
pub const ID_SIGNED_DATA: Oid = Oid::new_unwrap("1.2.840.113549.1.7.2");
/// id-envelopedData.
pub const ID_ENVELOPED_DATA: Oid = Oid::new_unwrap("1.2.840.113549.1.7.3");
/// id-ct-authEnvelopedData (RFC 5083).
pub const ID_AUTH_ENVELOPED_DATA: Oid = Oid::new_unwrap("1.2.840.113549.1.9.16.1.23");

// Signed attributes (RFC 5652 section 11, RFC 8551 section 2.5).
/// id-contentType.
pub const ID_CONTENT_TYPE: Oid = Oid::new_unwrap("1.2.840.113549.1.9.3");
/// id-messageDigest.
pub const ID_MESSAGE_DIGEST: Oid = Oid::new_unwrap("1.2.840.113549.1.9.4");
/// id-signingTime.
pub const ID_SIGNING_TIME: Oid = Oid::new_unwrap("1.2.840.113549.1.9.5");
/// smimeCapabilities.
pub const ID_SMIME_CAPABILITIES: Oid = Oid::new_unwrap("1.2.840.113549.1.9.15");

// Digests.
/// SHA-1 (accepted nowhere for signatures; used for thumbprints only).
pub const ID_SHA1: Oid = Oid::new_unwrap("1.3.14.3.2.26");
/// SHA-256.
pub const ID_SHA256: Oid = Oid::new_unwrap("2.16.840.1.101.3.4.2.1");
/// SHA-384.
pub const ID_SHA384: Oid = Oid::new_unwrap("2.16.840.1.101.3.4.2.2");
/// SHA-512.
pub const ID_SHA512: Oid = Oid::new_unwrap("2.16.840.1.101.3.4.2.3");
/// id-mgf1.
pub const ID_MGF1: Oid = Oid::new_unwrap("1.2.840.113549.1.1.8");

// Public keys and signatures.
/// rsaEncryption.
pub const RSA_ENCRYPTION: Oid = Oid::new_unwrap("1.2.840.113549.1.1.1");
/// id-RSAES-OAEP.
pub const RSAES_OAEP: Oid = Oid::new_unwrap("1.2.840.113549.1.1.7");
/// id-RSASSA-PSS.
pub const RSASSA_PSS: Oid = Oid::new_unwrap("1.2.840.113549.1.1.10");
/// sha1WithRSAEncryption.
pub const SHA1_WITH_RSA: Oid = Oid::new_unwrap("1.2.840.113549.1.1.5");
/// sha256WithRSAEncryption.
pub const SHA256_WITH_RSA: Oid = Oid::new_unwrap("1.2.840.113549.1.1.11");
/// sha384WithRSAEncryption.
pub const SHA384_WITH_RSA: Oid = Oid::new_unwrap("1.2.840.113549.1.1.12");
/// sha512WithRSAEncryption.
pub const SHA512_WITH_RSA: Oid = Oid::new_unwrap("1.2.840.113549.1.1.13");
/// id-ecPublicKey.
pub const EC_PUBLIC_KEY: Oid = Oid::new_unwrap("1.2.840.10045.2.1");
/// secp256r1 (P-256).
pub const SECP256R1: Oid = Oid::new_unwrap("1.2.840.10045.3.1.7");
/// secp384r1 (P-384).
pub const SECP384R1: Oid = Oid::new_unwrap("1.3.132.0.34");
/// secp521r1 (P-521; recognized, not supported).
pub const SECP521R1: Oid = Oid::new_unwrap("1.3.132.0.35");
/// ecdsa-with-SHA1.
pub const ECDSA_WITH_SHA1: Oid = Oid::new_unwrap("1.2.840.10045.4.1");
/// ecdsa-with-SHA256.
pub const ECDSA_WITH_SHA256: Oid = Oid::new_unwrap("1.2.840.10045.4.3.2");
/// ecdsa-with-SHA384.
pub const ECDSA_WITH_SHA384: Oid = Oid::new_unwrap("1.2.840.10045.4.3.3");
/// ecdsa-with-SHA512.
pub const ECDSA_WITH_SHA512: Oid = Oid::new_unwrap("1.2.840.10045.4.3.4");
/// id-Ed25519 (recognized, not supported).
pub const ED25519: Oid = Oid::new_unwrap("1.3.101.112");

// Content encryption and key wrap.
/// aes128-CBC.
pub const AES128_CBC: Oid = Oid::new_unwrap("2.16.840.1.101.3.4.1.2");
/// aes192-CBC.
pub const AES192_CBC: Oid = Oid::new_unwrap("2.16.840.1.101.3.4.1.22");
/// aes256-CBC.
pub const AES256_CBC: Oid = Oid::new_unwrap("2.16.840.1.101.3.4.1.42");
/// aes128-GCM.
pub const AES128_GCM: Oid = Oid::new_unwrap("2.16.840.1.101.3.4.1.6");
/// aes192-GCM.
pub const AES192_GCM: Oid = Oid::new_unwrap("2.16.840.1.101.3.4.1.26");
/// aes256-GCM.
pub const AES256_GCM: Oid = Oid::new_unwrap("2.16.840.1.101.3.4.1.46");
/// id-aes128-wrap.
pub const AES128_WRAP: Oid = Oid::new_unwrap("2.16.840.1.101.3.4.1.5");
/// id-aes192-wrap.
pub const AES192_WRAP: Oid = Oid::new_unwrap("2.16.840.1.101.3.4.1.25");
/// id-aes256-wrap.
pub const AES256_WRAP: Oid = Oid::new_unwrap("2.16.840.1.101.3.4.1.45");
/// des-ede3-cbc (recognized, not supported).
pub const DES_EDE3_CBC: Oid = Oid::new_unwrap("1.2.840.113549.3.7");
/// rc2-cbc (recognized, not supported).
pub const RC2_CBC: Oid = Oid::new_unwrap("1.2.840.113549.3.2");

// ECDH key agreement schemes (RFC 5753 section 7.1.4, RFC 8418 not
// supported).
/// dhSinglePass-stdDH-sha1kdf-scheme.
pub const ECDH_STD_SHA1_KDF: Oid = Oid::new_unwrap("1.3.133.16.840.63.0.2");
/// dhSinglePass-stdDH-sha224kdf-scheme.
pub const ECDH_STD_SHA224_KDF: Oid = Oid::new_unwrap("1.3.132.1.11.0");
/// dhSinglePass-stdDH-sha256kdf-scheme.
pub const ECDH_STD_SHA256_KDF: Oid = Oid::new_unwrap("1.3.132.1.11.1");
/// dhSinglePass-stdDH-sha384kdf-scheme.
pub const ECDH_STD_SHA384_KDF: Oid = Oid::new_unwrap("1.3.132.1.11.2");
/// dhSinglePass-stdDH-sha512kdf-scheme.
pub const ECDH_STD_SHA512_KDF: Oid = Oid::new_unwrap("1.3.132.1.11.3");

// Certificate extensions (RFC 5280).
/// id-ce-subjectKeyIdentifier.
pub const CE_SUBJECT_KEY_ID: Oid = Oid::new_unwrap("2.5.29.14");
/// id-ce-keyUsage.
pub const CE_KEY_USAGE: Oid = Oid::new_unwrap("2.5.29.15");
/// id-ce-subjectAltName.
pub const CE_SUBJECT_ALT_NAME: Oid = Oid::new_unwrap("2.5.29.17");
/// id-ce-issuerAltName.
pub const CE_ISSUER_ALT_NAME: Oid = Oid::new_unwrap("2.5.29.18");
/// id-ce-basicConstraints.
pub const CE_BASIC_CONSTRAINTS: Oid = Oid::new_unwrap("2.5.29.19");
/// id-ce-certificatePolicies.
pub const CE_CERTIFICATE_POLICIES: Oid = Oid::new_unwrap("2.5.29.32");
/// id-ce-authorityKeyIdentifier.
pub const CE_AUTHORITY_KEY_ID: Oid = Oid::new_unwrap("2.5.29.35");
/// id-ce-extKeyUsage.
pub const CE_EXT_KEY_USAGE: Oid = Oid::new_unwrap("2.5.29.37");
/// anyExtendedKeyUsage.
pub const ANY_EXTENDED_KEY_USAGE: Oid = Oid::new_unwrap("2.5.29.37.0");
/// id-kp-emailProtection.
pub const KP_EMAIL_PROTECTION: Oid = Oid::new_unwrap("1.3.6.1.5.5.7.3.4");
/// emailAddress (PKCS #9) in a subject name.
pub const EMAIL_ADDRESS: Oid = Oid::new_unwrap("1.2.840.113549.1.9.1");
/// commonName.
pub const COMMON_NAME: Oid = Oid::new_unwrap("2.5.4.3");
