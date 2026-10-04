//! The Windows certificate store (CryptoAPI / CNG through the `windows`
//! crate): listing and finding certificates of the current user's
//! "Personal" (`MY`) and "Other People" (`AddressBook`) stores, private-key
//! operations with keys that never leave the store (CNG `NCrypt*`), and
//! the system trust anchors (`Root`, intermediates from `CA`, distrust from
//! `Disallowed`).
//!
//! Nothing here writes to a system store. Every `unsafe` block of the crate
//! is in this module, each with its safety argument; each item that needs
//! one carries its own `#[expect(unsafe_code)]`.

use der::Encode;
use nail_api::security::{WindowsCertStore, WindowsCertificate};
use windows::Win32::Security::Cryptography::{
    BCRYPT_ECCKEY_BLOB, BCRYPT_ECCPUBLIC_BLOB, BCRYPT_ECDH_PUBLIC_P256_MAGIC,
    BCRYPT_ECDH_PUBLIC_P384_MAGIC, BCRYPT_KDF_RAW_SECRET, BCRYPT_OAEP_PADDING_INFO,
    BCRYPT_PKCS1_PADDING_INFO, BCRYPT_SHA1_ALGORITHM, BCRYPT_SHA256_ALGORITHM,
    BCRYPT_SHA384_ALGORITHM, BCRYPT_SHA512_ALGORITHM, CERT_CONTEXT, CERT_FIND_SHA1_HASH,
    CERT_FIND_SUBJECT_NAME, CERT_KEY_CONTEXT_PROP_ID, CERT_KEY_PROV_INFO_PROP_ID, CERT_KEY_SPEC,
    CERT_NCRYPT_KEY_HANDLE_PROP_ID, CERT_NCRYPT_KEY_SPEC, CERT_OPEN_STORE_FLAGS,
    CERT_QUERY_ENCODING_TYPE, CERT_STORE_OPEN_EXISTING_FLAG, CERT_STORE_PROV_SYSTEM_W,
    CERT_STORE_READONLY_FLAG, CERT_SYSTEM_STORE_CURRENT_USER, CRYPT_ACQUIRE_CACHE_FLAG,
    CRYPT_ACQUIRE_FLAGS, CRYPT_ACQUIRE_ONLY_NCRYPT_KEY_FLAG, CRYPT_ACQUIRE_SILENT_FLAG,
    CRYPT_INTEGER_BLOB, CertCloseStore, CertDuplicateCertificateContext,
    CertEnumCertificatesInStore, CertFindCertificateInStore, CertFreeCertificateContext,
    CertGetCertificateContextProperty, CertOpenStore, CryptAcquireCertificatePrivateKey,
    HCERTSTORE, HCRYPTPROV_OR_NCRYPT_KEY_HANDLE, NCRYPT_FLAGS, NCRYPT_HANDLE, NCRYPT_KEY_HANDLE,
    NCRYPT_PAD_OAEP_FLAG, NCRYPT_PAD_PKCS1_FLAG, NCRYPT_PROV_HANDLE,
    NCRYPT_PROVIDER_HANDLE_PROPERTY, NCRYPT_SECRET_HANDLE, NCryptDecrypt, NCryptDeriveKey,
    NCryptFreeObject, NCryptGetProperty, NCryptImportKey, NCryptSecretAgreement, NCryptSignHash,
    PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
};
use windows::core::{BOOL, PCWSTR};
use x509_cert::name::Name;
use zeroize::Zeroizing;

use crate::cert::{Cert, Purpose};
use crate::chain::TrustAnchors;
use crate::error::{Error, Result};
use crate::keys::{HashAlg, PrivateKeyOps, RsaPadding, SignScheme};

const ENCODING: CERT_QUERY_ENCODING_TYPE =
    CERT_QUERY_ENCODING_TYPE(X509_ASN_ENCODING.0 | PKCS_7_ASN_ENCODING.0);

fn win_err(what: &str, err: &windows::core::Error) -> Error {
    Error::Windows(format!("{what}: {err}"))
}

fn wide(text: &str) -> Vec<u16> {
    text.encode_utf16().chain(std::iter::once(0)).collect()
}

/// An open certificate store, closed on drop.
pub(crate) struct StoreHandle(HCERTSTORE);

// SAFETY: CryptoAPI certificate store handles may be used from any thread
// (the store serializes access internally); the handle is only closed in
// `Drop`, once.
#[expect(
    unsafe_code,
    reason = "store handles are thread-safe; see the SAFETY note"
)]
unsafe impl Send for StoreHandle {}
// SAFETY: see `Send`; every method takes `&self` and calls thread-safe
// CryptoAPI functions.
#[expect(
    unsafe_code,
    reason = "store handles are thread-safe; see the SAFETY note"
)]
unsafe impl Sync for StoreHandle {}

impl Drop for StoreHandle {
    #[expect(unsafe_code, reason = "closes the CryptoAPI store")]
    fn drop(&mut self) {
        // SAFETY: the handle came from CertOpenStore / PFXImportCertStore
        // and is closed exactly once here.
        unsafe {
            drop(CertCloseStore(Some(self.0), 0));
        }
    }
}

/// An owned certificate context, freed on drop.
pub(crate) struct Context(*const CERT_CONTEXT);

impl Drop for Context {
    #[expect(unsafe_code, reason = "frees the certificate context")]
    fn drop(&mut self) {
        // SAFETY: the context was duplicated or returned by a find call and
        // is owned by this value; it is freed exactly once.
        unsafe {
            let _ = CertFreeCertificateContext(Some(self.0));
        }
    }
}

impl Context {
    #[expect(
        unsafe_code,
        reason = "reads the encoded certificate of a live context"
    )]
    fn der(&self) -> &[u8] {
        // SAFETY: a live CERT_CONTEXT (owned by self) points to
        // `cbCertEncoded` bytes of DER that live as long as the context.
        unsafe {
            let ctx = &*self.0;
            std::slice::from_raw_parts(ctx.pbCertEncoded, ctx.cbCertEncoded as usize)
        }
    }

    #[expect(unsafe_code, reason = "queries a property of a live context")]
    fn has_property(&self, id: u32) -> bool {
        let mut size = 0u32;
        // SAFETY: a size query (no output buffer) on a live context.
        unsafe { CertGetCertificateContextProperty(self.0, id, None, &raw mut size).is_ok() }
    }

    /// Whether a private key is associated (persisted CSP / KSP key, or an
    /// in-memory CNG key).
    fn has_private_key(&self) -> bool {
        [
            CERT_KEY_PROV_INFO_PROP_ID,
            CERT_NCRYPT_KEY_HANDLE_PROP_ID,
            CERT_KEY_CONTEXT_PROP_ID,
        ]
        .into_iter()
        .any(|id| self.has_property(id))
    }
}

impl StoreHandle {
    /// A current-user system store, read-only. `None` when it does not
    /// exist.
    #[expect(unsafe_code, reason = "CertOpenStore")]
    fn system(name: &str) -> Option<Self> {
        let name = wide(name);
        let flags = CERT_OPEN_STORE_FLAGS(
            CERT_SYSTEM_STORE_CURRENT_USER
                | CERT_STORE_READONLY_FLAG.0
                | CERT_STORE_OPEN_EXISTING_FLAG.0,
        );
        // SAFETY: `name` is a NUL-terminated UTF-16 string that outlives
        // the call; the returned handle is owned by StoreHandle.
        let store = unsafe {
            CertOpenStore(
                CERT_STORE_PROV_SYSTEM_W,
                CERT_QUERY_ENCODING_TYPE(0),
                None,
                flags,
                Some(name.as_ptr().cast()),
            )
        };
        store.ok().map(Self)
    }

    /// An empty in-memory store.
    #[cfg(test)]
    #[expect(unsafe_code, reason = "CertOpenStore")]
    fn memory() -> Result<Self> {
        use windows::Win32::Security::Cryptography::CERT_STORE_PROV_MEMORY;
        // SAFETY: the memory provider takes no parameter; the handle is
        // owned by StoreHandle.
        let store = unsafe {
            CertOpenStore(
                CERT_STORE_PROV_MEMORY,
                CERT_QUERY_ENCODING_TYPE(0),
                None,
                CERT_OPEN_STORE_FLAGS(0),
                None,
            )
        }
        .map_err(|e| win_err("open memory store", &e))?;
        Ok(Self(store))
    }

    /// A PKCS#12 file opened into memory with ephemeral CNG keys (nothing
    /// is persisted): lets tests use real CNG keys without touching the
    /// user's stores.
    #[cfg(test)]
    #[expect(unsafe_code, reason = "PFXImportCertStore")]
    fn from_pfx(data: &[u8], password: &str) -> Result<Self> {
        use windows::Win32::Security::Cryptography::{
            CRYPT_KEY_FLAGS, PFXImportCertStore, PKCS12_ALWAYS_CNG_KSP, PKCS12_NO_PERSIST_KEY,
        };
        let blob = CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(data.len()).map_err(|_| Error::malformed("PFX too large"))?,
            pbData: data.as_ptr().cast_mut(),
        };
        let password = wide(password);
        // SAFETY: `blob` points to `data`, which outlives the call and is
        // only read; `password` is NUL-terminated UTF-16.
        let store = unsafe {
            PFXImportCertStore(
                &raw const blob,
                PCWSTR(password.as_ptr()),
                CRYPT_KEY_FLAGS(PKCS12_NO_PERSIST_KEY.0 | PKCS12_ALWAYS_CNG_KSP.0),
            )
        }
        .map_err(|e| win_err("import PFX", &e))?;
        Ok(Self(store))
    }

    /// Adds a DER certificate (tests).
    #[cfg(test)]
    #[expect(unsafe_code, reason = "CertAddEncodedCertificateToStore")]
    fn add(&self, der: &[u8]) -> Result<()> {
        use windows::Win32::Security::Cryptography::{
            CERT_STORE_ADD_REPLACE_EXISTING, CertAddEncodedCertificateToStore,
        };
        // SAFETY: `der` is read during the call only; no context is
        // returned.
        unsafe {
            CertAddEncodedCertificateToStore(
                Some(self.0),
                ENCODING,
                der,
                CERT_STORE_ADD_REPLACE_EXISTING,
                None,
            )
        }
        .map_err(|e| win_err("add certificate", &e))
    }

    /// Every certificate in the store.
    #[expect(unsafe_code, reason = "CertEnumCertificatesInStore")]
    fn contexts(&self) -> Vec<Context> {
        let mut out = Vec::new();
        // SAFETY: enumeration hands back contexts owned by the enumeration;
        // each is duplicated before the next call frees it, and the
        // duplicate is owned by `Context`.
        unsafe {
            let mut current = CertEnumCertificatesInStore(self.0, None);
            while !current.is_null() {
                out.push(Context(CertDuplicateCertificateContext(Some(current))));
                current = CertEnumCertificatesInStore(self.0, Some(current));
            }
        }
        out
    }

    /// The certificate with this SHA-1 thumbprint.
    #[expect(unsafe_code, reason = "CertFindCertificateInStore")]
    fn find_sha1(&self, thumbprint: &[u8]) -> Option<Context> {
        let blob = CRYPT_INTEGER_BLOB {
            cbData: u32::try_from(thumbprint.len()).ok()?,
            pbData: thumbprint.as_ptr().cast_mut(),
        };
        // SAFETY: `blob` points to `thumbprint`, read during the call; the
        // returned context (or null) is owned by `Context`.
        let found = unsafe {
            CertFindCertificateInStore(
                self.0,
                ENCODING,
                0,
                CERT_FIND_SHA1_HASH,
                Some((&raw const blob).cast()),
                None,
            )
        };
        (!found.is_null()).then_some(Context(found))
    }

    /// Certificates whose subject is exactly `name`.
    #[expect(unsafe_code, reason = "CertFindCertificateInStore")]
    fn find_subject(&self, name: &Name) -> Vec<Context> {
        let Ok(der) = name.to_der() else {
            return Vec::new();
        };
        let Ok(len) = u32::try_from(der.len()) else {
            return Vec::new();
        };
        let blob = CRYPT_INTEGER_BLOB {
            cbData: len,
            pbData: der.as_ptr().cast_mut(),
        };
        let mut out = Vec::new();
        // SAFETY: `blob` points to `der`, alive for the loop; each found
        // context is duplicated before being passed back as `prev` (which
        // the API frees), and the duplicate is owned by `Context`.
        unsafe {
            let mut previous: Option<*const CERT_CONTEXT> = None;
            loop {
                let found = CertFindCertificateInStore(
                    self.0,
                    ENCODING,
                    0,
                    CERT_FIND_SUBJECT_NAME,
                    Some((&raw const blob).cast()),
                    previous,
                );
                if found.is_null() {
                    break;
                }
                out.push(Context(CertDuplicateCertificateContext(Some(found))));
                previous = Some(found);
            }
        }
        out
    }
}

fn store_name(store: WindowsCertStore) -> &'static str {
    match store {
        WindowsCertStore::Personal => "MY",
        WindowsCertStore::OtherPeople => "AddressBook",
    }
}

fn parse_thumbprint(thumbprint: &str) -> Result<Vec<u8>> {
    let hex: String = thumbprint.chars().filter(|c| !c.is_whitespace()).collect();
    if hex.len() != 40 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::malformed("a thumbprint is 40 hexadecimal digits"));
    }
    Ok((0..40)
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).unwrap_or(0))
        .collect())
}

fn describe(store: WindowsCertStore, context: &Context) -> Option<WindowsCertificate> {
    let cert = Cert::from_der(context.der()).ok()?;
    Some(WindowsCertificate {
        store,
        thumbprint: cert.thumbprint(),
        certificate: cert.summary(),
        emails: cert.emails(),
        has_private_key: context.has_private_key(),
        usable_for_email: !cert.is_ca() && cert.usable_for(Purpose::Any),
        imported: None,
    })
}

fn list_in(handle: &StoreHandle, store: WindowsCertStore) -> Vec<WindowsCertificate> {
    let mut list: Vec<WindowsCertificate> = handle
        .contexts()
        .iter()
        .filter_map(|c| describe(store, c))
        .collect();
    list.sort_by(|a, b| {
        (!a.usable_for_email, &a.certificate.subject)
            .cmp(&(!b.usable_for_email, &b.certificate.subject))
    });
    list
}

/// The certificates of a current-user store (mail certificates first);
/// empty when the store does not exist.
pub fn list_certificates(store: WindowsCertStore) -> Result<Vec<WindowsCertificate>> {
    Ok(match StoreHandle::system(store_name(store)) {
        Some(handle) => list_in(&handle, store),
        None => Vec::new(),
    })
}

/// A certificate found in a Windows store.
#[derive(Debug, Clone)]
pub struct FoundCertificate {
    /// The certificate.
    pub cert: Cert,
    /// Whether the store holds its private key.
    pub has_private_key: bool,
}

fn find_in(handle: &StoreHandle, thumbprint: &str) -> Result<FoundCertificate> {
    let hash = parse_thumbprint(thumbprint)?;
    let context = handle.find_sha1(&hash).ok_or_else(|| {
        Error::Store(nail_store::Error::NotFound {
            kind: "Windows certificate",
            id: thumbprint.to_owned(),
        })
    })?;
    Ok(FoundCertificate {
        cert: Cert::from_der(context.der())?,
        has_private_key: context.has_private_key(),
    })
}

/// Finds a certificate by its SHA-1 thumbprint.
pub fn find_certificate(store: WindowsCertStore, thumbprint: &str) -> Result<FoundCertificate> {
    let handle = StoreHandle::system(store_name(store)).ok_or_else(|| {
        Error::Store(nail_store::Error::NotFound {
            kind: "Windows certificate store",
            id: store_name(store).to_owned(),
        })
    })?;
    find_in(&handle, thumbprint)
}

enum KeyLocation {
    System(WindowsCertStore),
    #[cfg(test)]
    Handle(std::sync::Arc<StoreHandle>),
}

/// A private key held by the Windows certificate store; every operation
/// opens it through CNG, so it never enters Nail's memory.
pub struct WindowsKey {
    location: KeyLocation,
    thumbprint: Vec<u8>,
    silent: bool,
}

impl std::fmt::Debug for WindowsKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "WindowsKey({})",
            crate::cert::hex_upper(&self.thumbprint)
        )
    }
}

/// Frees an NCrypt object on drop.
struct NcryptObject(usize);

impl Drop for NcryptObject {
    #[expect(unsafe_code, reason = "frees the NCrypt object")]
    fn drop(&mut self) {
        if self.0 != 0 {
            // SAFETY: the handle was created by NCrypt for this value and is
            // freed exactly once.
            unsafe {
                drop(NCryptFreeObject(NCRYPT_HANDLE(self.0)));
            }
        }
    }
}

impl WindowsKey {
    /// The key of a certificate in the "Personal" store. Windows may show
    /// its own prompt (a smart card PIN, strong key protection).
    pub fn personal(thumbprint: &str) -> Result<Self> {
        Ok(Self {
            location: KeyLocation::System(WindowsCertStore::Personal),
            thumbprint: parse_thumbprint(thumbprint)?,
            silent: false,
        })
    }

    #[cfg(test)]
    fn in_handle(handle: std::sync::Arc<StoreHandle>, thumbprint: &str) -> Result<Self> {
        Ok(Self {
            location: KeyLocation::Handle(handle),
            thumbprint: parse_thumbprint(thumbprint)?,
            silent: true,
        })
    }

    #[expect(unsafe_code, reason = "CryptAcquireCertificatePrivateKey")]
    fn with_key<T>(&self, f: impl FnOnce(NCRYPT_KEY_HANDLE) -> Result<T>) -> Result<T> {
        let opened;
        let handle: &StoreHandle = match &self.location {
            KeyLocation::System(store) => {
                opened = StoreHandle::system(store_name(*store))
                    .ok_or_else(|| Error::NoKey("the Windows store is missing".into()))?;
                &opened
            }
            #[cfg(test)]
            KeyLocation::Handle(handle) => handle,
        };
        let context = handle
            .find_sha1(&self.thumbprint)
            .ok_or_else(|| Error::NoKey("the certificate left the Windows store".into()))?;
        let mut key = HCRYPTPROV_OR_NCRYPT_KEY_HANDLE::default();
        let mut spec = CERT_KEY_SPEC(0);
        let mut must_free = BOOL(0);
        // CACHE: reuse the handle attached to the context (the only way to
        // reach an in-memory key; for persisted keys the handle is released
        // with the context at the end of this call).
        let mut flags = CRYPT_ACQUIRE_ONLY_NCRYPT_KEY_FLAG.0 | CRYPT_ACQUIRE_CACHE_FLAG.0;
        if self.silent {
            flags |= CRYPT_ACQUIRE_SILENT_FLAG.0;
        }
        // SAFETY: a live context; the out-pointers are valid locals. The key
        // handle is released below when `must_free` says so.
        unsafe {
            CryptAcquireCertificatePrivateKey(
                context.0,
                CRYPT_ACQUIRE_FLAGS(flags),
                None,
                &raw mut key,
                Some(&raw mut spec),
                Some(&raw mut must_free),
            )
        }
        .map_err(|e| Error::NoKey(format!("Windows private key: {e}")))?;
        let _guard = must_free.as_bool().then(|| NcryptObject(key.0));
        if spec != CERT_NCRYPT_KEY_SPEC {
            return Err(Error::Unsupported("a legacy CryptoAPI key".into()));
        }
        f(NCRYPT_KEY_HANDLE(key.0))
    }
}

fn hash_id(hash: HashAlg) -> PCWSTR {
    match hash {
        HashAlg::Sha1 => BCRYPT_SHA1_ALGORITHM,
        HashAlg::Sha256 => BCRYPT_SHA256_ALGORITHM,
        HashAlg::Sha384 => BCRYPT_SHA384_ALGORITHM,
        HashAlg::Sha512 => BCRYPT_SHA512_ALGORITHM,
    }
}

/// NCrypt's two-call pattern: size, then data.
fn two_call(
    what: &str,
    mut call: impl FnMut(Option<&mut [u8]>, &mut u32) -> windows::core::Result<()>,
) -> Result<Vec<u8>> {
    let mut size = 0u32;
    call(None, &mut size).map_err(|e| win_err(what, &e))?;
    let mut out = vec![0u8; size as usize];
    call(Some(&mut out), &mut size).map_err(|e| win_err(what, &e))?;
    out.truncate(size as usize);
    Ok(out)
}

impl PrivateKeyOps for WindowsKey {
    #[expect(unsafe_code, reason = "NCryptSignHash")]
    fn sign_digest(&self, scheme: SignScheme, digest: &[u8]) -> Result<Vec<u8>> {
        self.with_key(|key| match scheme {
            SignScheme::RsaPkcs1(hash) => {
                let padding = BCRYPT_PKCS1_PADDING_INFO {
                    pszAlgId: hash_id(hash),
                };
                two_call("sign", |out, size| {
                    // SAFETY: `padding` and `digest` outlive the call; `out`
                    // is a buffer of the reported size or None.
                    unsafe {
                        NCryptSignHash(
                            key,
                            Some((&raw const padding).cast()),
                            digest,
                            out,
                            size,
                            NCRYPT_PAD_PKCS1_FLAG,
                        )
                    }
                })
            }
            SignScheme::Ecdsa(_) => {
                let raw = two_call("sign", |out, size| {
                    // SAFETY: as above, without padding information (ECDSA).
                    unsafe { NCryptSignHash(key, None, digest, out, size, NCRYPT_FLAGS(0)) }
                })?;
                // CNG returns r || s; CMS wants the DER SEQUENCE.
                match raw.len() {
                    64 => Ok(p256::ecdsa::Signature::from_slice(&raw)
                        .map_err(|_| Error::Crypto("bad ECDSA signature".into()))?
                        .to_der()
                        .as_bytes()
                        .to_vec()),
                    96 => Ok(p384::ecdsa::Signature::from_slice(&raw)
                        .map_err(|_| Error::Crypto("bad ECDSA signature".into()))?
                        .to_der()
                        .as_bytes()
                        .to_vec()),
                    _ => Err(Error::Unsupported("ECDSA curve of the Windows key".into())),
                }
            }
            SignScheme::RsaPss { .. } => Err(Error::Unsupported("signing with RSA-PSS".into())),
        })
    }

    #[expect(unsafe_code, reason = "NCryptDecrypt")]
    fn decrypt_rsa(&self, padding: RsaPadding, ciphertext: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        self.with_key(|key| {
            let plain = match padding {
                RsaPadding::Pkcs1v15 => two_call("decrypt", |out, size| {
                    // SAFETY: `ciphertext` outlives the call; `out` is a
                    // buffer of the reported size or None.
                    unsafe {
                        NCryptDecrypt(
                            key,
                            Some(ciphertext),
                            None,
                            out,
                            size,
                            NCRYPT_PAD_PKCS1_FLAG,
                        )
                    }
                }),
                RsaPadding::Oaep(hash) => {
                    let info = BCRYPT_OAEP_PADDING_INFO {
                        pszAlgId: hash_id(hash),
                        pbLabel: std::ptr::null_mut(),
                        cbLabel: 0,
                    };
                    two_call("decrypt", |out, size| {
                        // SAFETY: as above; `info` outlives the call.
                        unsafe {
                            NCryptDecrypt(
                                key,
                                Some(ciphertext),
                                Some((&raw const info).cast()),
                                out,
                                size,
                                NCRYPT_PAD_OAEP_FLAG,
                            )
                        }
                    })
                }
            };
            plain
                .map(Zeroizing::new)
                .map_err(|_| Error::Crypto("key transport decryption failed".into()))
        })
    }

    #[expect(
        unsafe_code,
        reason = "NCryptImportKey / NCryptSecretAgreement / NCryptDeriveKey"
    )]
    fn ecdh(&self, peer_point: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        let (magic, coord) = match peer_point.len() {
            65 => (BCRYPT_ECDH_PUBLIC_P256_MAGIC, 32u32),
            97 => (BCRYPT_ECDH_PUBLIC_P384_MAGIC, 48u32),
            _ => return Err(Error::Unsupported("ECDH curve".into())),
        };
        if peer_point[0] != 4 {
            return Err(Error::malformed("compressed originator point"));
        }
        // The 8-byte header, then X and Y (the point without its 0x04).
        let mut blob = Vec::with_capacity(8 + peer_point.len() - 1);
        let header = BCRYPT_ECCKEY_BLOB {
            dwMagic: magic,
            cbKey: coord,
        };
        blob.extend_from_slice(&header.dwMagic.to_le_bytes());
        blob.extend_from_slice(&header.cbKey.to_le_bytes());
        blob.extend_from_slice(&peer_point[1..]);
        self.with_key(|key| {
            let provider = two_call("provider", |out, size| {
                // SAFETY: a property read into a buffer of the reported size.
                unsafe {
                    NCryptGetProperty(
                        NCRYPT_HANDLE(key.0),
                        NCRYPT_PROVIDER_HANDLE_PROPERTY,
                        out,
                        size,
                        windows::Win32::Security::OBJECT_SECURITY_INFORMATION(0),
                    )
                }
            })?;
            let provider = usize::from_le_bytes(
                provider
                    .get(..size_of::<usize>())
                    .and_then(|b| b.try_into().ok())
                    .ok_or_else(|| Error::Windows("bad provider handle".into()))?,
            );
            let _provider = NcryptObject(provider);
            let mut public = NCRYPT_KEY_HANDLE::default();
            // SAFETY: `blob` is a well-formed BCRYPT_ECCPUBLIC_BLOB that
            // outlives the call; the new key handle is freed by the guard.
            unsafe {
                NCryptImportKey(
                    NCRYPT_PROV_HANDLE(provider),
                    None,
                    BCRYPT_ECCPUBLIC_BLOB,
                    None,
                    &raw mut public,
                    &blob,
                    NCRYPT_FLAGS(0),
                )
            }
            .map_err(|e| win_err("import originator key", &e))?;
            let _public = NcryptObject(public.0);
            let mut secret = NCRYPT_SECRET_HANDLE::default();
            // SAFETY: both key handles are live; the secret handle is freed
            // by the guard.
            unsafe { NCryptSecretAgreement(key, public, &raw mut secret, NCRYPT_FLAGS(0)) }
                .map_err(|e| Error::Unsupported(format!("ECDH with this Windows key: {e}")))?;
            let _secret = NcryptObject(secret.0);
            let mut raw = two_call("derive", |out, size| {
                // SAFETY: a live secret handle; `out` is a buffer of the
                // reported size or None.
                unsafe { NCryptDeriveKey(secret, BCRYPT_KDF_RAW_SECRET, None, out, size, 0) }
            })?;
            // CNG returns the raw secret little-endian.
            raw.reverse();
            Ok(Zeroizing::new(raw))
        })
    }
}

/// The Windows trust store: roots from the current user's `Root` store
/// (which includes the machine's), intermediates from `CA`, distrust from
/// `Disallowed`.
#[derive(Debug, Default, Clone, Copy)]
pub struct WindowsTrustAnchors;

fn certs_by_subject(store: &str, subject: &Name) -> Vec<Cert> {
    match StoreHandle::system(store) {
        Some(handle) => handle
            .find_subject(subject)
            .iter()
            .filter_map(|c| Cert::from_der(c.der()).ok())
            .collect(),
        None => Vec::new(),
    }
}

impl TrustAnchors for WindowsTrustAnchors {
    fn anchors_for(&self, subject: &Name) -> Vec<Cert> {
        certs_by_subject("Root", subject)
    }

    fn intermediates_for(&self, subject: &Name) -> Vec<Cert> {
        certs_by_subject("CA", subject)
    }

    fn is_distrusted(&self, cert: &Cert) -> bool {
        let Ok(hash) = parse_thumbprint(&cert.thumbprint()) else {
            return false;
        };
        StoreHandle::system("Disallowed").is_some_and(|h| h.find_sha1(&hash).is_some())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::chain;
    use crate::enveloped::{self, ContentCipher, Decryption, DecryptionKey};
    use crate::signed::{self, Signer, VerifyContext};
    use crate::testutil::{AT, P12_PASSWORD, fixture, fixture_cert, test_anchors};
    use nail_api::security::{ChainState, SignatureState};

    use std::sync::Arc;

    fn pfx(name: &str) -> Arc<StoreHandle> {
        Arc::new(StoreHandle::from_pfx(&fixture(name), P12_PASSWORD).unwrap())
    }

    #[test]
    fn system_stores_open_and_list_without_changes() {
        // The user's stores may be empty; listing must work either way.
        let personal = list_certificates(WindowsCertStore::Personal).unwrap();
        for item in &personal {
            assert_eq!(item.thumbprint.len(), 40);
            assert_eq!(item.store, WindowsCertStore::Personal);
        }
        list_certificates(WindowsCertStore::OtherPeople).unwrap();
    }

    #[test]
    fn memory_store_listing_and_lookup() {
        let store = StoreHandle::memory().unwrap();
        for name in ["alice.crt", "intermediate-ca.crt", "dave.crt"] {
            store.add(fixture_cert(name).der()).unwrap();
        }
        let list = list_in(&store, WindowsCertStore::OtherPeople);
        assert_eq!(list.len(), 3);
        assert!(list[0].usable_for_email);
        assert_eq!(list[0].emails, ["alice@example.org"]);
        assert!(!list[0].has_private_key);
        assert_eq!(list.iter().filter(|c| !c.usable_for_email).count(), 2);
        let alice = fixture_cert("alice.crt");
        let found = find_in(&store, &alice.thumbprint().to_lowercase()).unwrap();
        assert_eq!(found.cert, alice);
        let missing = find_in(&store, &"00".repeat(20)).unwrap_err();
        assert_eq!(missing.code(), nail_api::ErrorCode::NotFound);
        assert!(find_in(&store, "xyz").is_err());
        let intermediate = fixture_cert("intermediate-ca.crt");
        let subject = &intermediate.inner().tbs_certificate.subject;
        assert_eq!(store.find_subject(subject).len(), 1);
    }

    #[test]
    fn cng_rsa_key_signs_and_decrypts() {
        let store = pfx("alice.p12");
        let alice = fixture_cert("alice.crt");
        let list = list_in(&store, WindowsCertStore::Personal);
        let entry = list
            .iter()
            .find(|c| c.thumbprint == alice.thumbprint())
            .unwrap();
        assert!(entry.has_private_key);
        let key = WindowsKey::in_handle(store.clone(), &alice.thumbprint()).unwrap();
        assert!(format!("{key:?}").starts_with("WindowsKey("));

        let chain = [fixture_cert("intermediate-ca.crt")];
        let signer = Signer {
            key: &key,
            cert: &alice,
            chain: &chain,
        };
        let der = signed::sign(b"signed by CNG", &signer, false, AT).unwrap();
        let anchors = test_anchors();
        let verified = signed::verify(
            &der,
            None,
            &VerifyContext {
                anchors: &anchors,
                known_certs: &[],
                from: None,
                now: AT,
            },
        )
        .unwrap();
        assert_eq!(verified.signers[0].info.state, SignatureState::Valid);

        for cipher in [ContentCipher::Aes256Gcm, ContentCipher::Aes128Cbc] {
            let der = enveloped::encrypt(b"for CNG", std::slice::from_ref(&alice), cipher).unwrap();
            let (_, d) = enveloped::decrypt(
                &der,
                &[DecryptionKey {
                    cert: &alice,
                    key: Some(&key),
                }],
            )
            .unwrap();
            let Decryption::Decrypted { plaintext, .. } = d else {
                panic!("not decrypted")
            };
            assert_eq!(plaintext.as_slice(), b"for CNG");
        }
        // RSA-OAEP from OpenSSL.
        let message = fixture("alice-enc-oaep.eml");
        let body = crate::mime::decode_body(&message).unwrap();
        let (_, d) = enveloped::decrypt(
            &body,
            &[DecryptionKey {
                cert: &alice,
                key: Some(&key),
            }],
        )
        .unwrap();
        assert!(matches!(d, Decryption::Decrypted { .. }));
    }

    #[test]
    fn cng_ec_key_signs_and_agrees() {
        let store = pfx("bob.p12");
        let bob = fixture_cert("bob.crt");
        let key = WindowsKey::in_handle(store, &bob.thumbprint()).unwrap();
        let signer = Signer {
            key: &key,
            cert: &bob,
            chain: &[],
        };
        let der = signed::sign(b"ECDSA by CNG", &signer, true, AT).unwrap();
        let anchors = test_anchors();
        let known = [fixture_cert("intermediate-ca.crt")];
        let verified = signed::verify(
            &der,
            Some(b"ECDSA by CNG"),
            &VerifyContext {
                anchors: &anchors,
                known_certs: &known,
                from: Some("bob@example.net"),
                now: AT,
            },
        )
        .unwrap();
        assert_eq!(verified.signers[0].info.state, SignatureState::Valid);
        // ECDH through CNG against Nail's own software ECDH.
        let der = enveloped::encrypt(
            b"for CNG ECDH",
            std::slice::from_ref(&bob),
            ContentCipher::Aes256Gcm,
        )
        .unwrap();
        let (_, d) = enveloped::decrypt(
            &der,
            &[DecryptionKey {
                cert: &bob,
                key: Some(&key),
            }],
        )
        .unwrap();
        let Decryption::Decrypted { plaintext, .. } = d else {
            panic!("not decrypted")
        };
        assert_eq!(plaintext.as_slice(), b"for CNG ECDH");
    }

    #[test]
    fn missing_certificate_is_no_key() {
        let store = Arc::new(StoreHandle::memory().unwrap());
        let key = WindowsKey::in_handle(store, &"AB".repeat(20)).unwrap();
        assert!(matches!(
            key.sign_digest(SignScheme::Ecdsa(HashAlg::Sha256), &[0; 32]),
            Err(Error::NoKey(_))
        ));
        // A certificate without a private key.
        let store = StoreHandle::memory().unwrap();
        let alice = fixture_cert("alice.crt");
        store.add(alice.der()).unwrap();
        let key = WindowsKey::in_handle(Arc::new(store), &alice.thumbprint()).unwrap();
        assert!(matches!(
            key.decrypt_rsa(RsaPadding::Pkcs1v15, &[0; 256]),
            Err(Error::NoKey(_))
        ));
        assert!(WindowsKey::personal("not hex").is_err());
    }

    #[test]
    fn system_trust_anchors_find_installed_roots() {
        let Some(root_store) = StoreHandle::system("Root") else {
            panic!("every Windows installation has a Root store");
        };
        let roots = root_store.contexts();
        let root = roots
            .iter()
            .find_map(|c| Cert::from_der(c.der()).ok())
            .expect("at least one trusted root");
        let anchors = WindowsTrustAnchors;
        let subject = root.inner().tbs_certificate.subject.clone();
        assert!(anchors.anchors_for(&subject).contains(&root));
        assert!(anchors.is_anchor(&root));
        assert!(!anchors.is_distrusted(&root));
        // The Nail test root is not installed: its chain is untrusted.
        let test_root = fixture_cert("root-ca.crt");
        assert!(!anchors.is_anchor(&test_root));
        let v = chain::validate(
            &fixture_cert("alice.crt"),
            &[fixture_cert("intermediate-ca.crt"), test_root],
            &anchors,
            Purpose::Sign,
            AT,
        );
        assert_eq!(v.state(), ChainState::UntrustedRoot);
    }
}
