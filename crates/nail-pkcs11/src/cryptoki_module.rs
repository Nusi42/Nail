//! [`Module`] over a vendor PKCS#11 DLL through `cryptoki`.
//!
//! One read-only session per slot, opened on first use and kept (so a
//! login lasts as long as the PIN cache allows); objects are found by
//! class and `CKA_ID`. No key material leaves the card except public
//! attributes and the ECDH shared secret (derived as an extractable
//! session object, read and destroyed).

use std::collections::{HashMap, HashSet};
use std::sync::{Mutex, MutexGuard, PoisonError};

use cryptoki::context::{CInitializeArgs, CInitializeFlags, Pkcs11};
use cryptoki::error::{Error as CkError, RvError};
use cryptoki::mechanism::elliptic_curve::{EcKdf, Ecdh1DeriveParams};
use cryptoki::mechanism::rsa::{PkcsMgfType, PkcsOaepParams, PkcsOaepSource, PkcsPssParams};
use cryptoki::mechanism::{Mechanism, MechanismType};
use cryptoki::object::{Attribute, AttributeType, KeyType, ObjectClass, ObjectHandle};
use cryptoki::session::{Session, UserType};
use cryptoki::slot::Slot;
use cryptoki::types::AuthPin;
use nail_api::smartcard::{SmartcardError, SmartcardObjectKind};
use zeroize::Zeroizing;

use crate::error::{CardError, from_cryptoki, needs_login};
use crate::module::{
    DecryptMechanism, HashKind, Module, ObjectInfo, PublicKeyMaterial, SignMechanism, TokenInfo,
};

/// A loaded PKCS#11 module.
pub struct CryptokiModule {
    path: String,
    ctx: Pkcs11,
    sessions: Mutex<HashMap<u64, Session>>,
    logged_in: Mutex<HashSet<u64>>,
}

impl std::fmt::Debug for CryptokiModule {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CryptokiModule")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

fn slot(slot_id: u64) -> Result<Slot, CardError> {
    Slot::try_from(slot_id).map_err(|e| from_cryptoki(&e))
}

fn hash_mechanism(hash: HashKind) -> (MechanismType, PkcsMgfType) {
    match hash {
        HashKind::Sha1 => (MechanismType::SHA1, PkcsMgfType::MGF1_SHA1),
        HashKind::Sha256 => (MechanismType::SHA256, PkcsMgfType::MGF1_SHA256),
        HashKind::Sha384 => (MechanismType::SHA384, PkcsMgfType::MGF1_SHA384),
        HashKind::Sha512 => (MechanismType::SHA512, PkcsMgfType::MGF1_SHA512),
    }
}

fn text(bytes: &[u8]) -> String {
    String::from_utf8_lossy(bytes)
        .trim_end_matches(['\0', ' '])
        .to_owned()
}

impl CryptokiModule {
    /// Loads and initializes the module at `path`.
    pub fn load(path: &str) -> Result<Self, CardError> {
        if !std::path::Path::new(path).is_file() {
            return Err(CardError::new(
                SmartcardError::ModuleNotFound,
                format!("{path} does not exist"),
            ));
        }
        let ctx = Pkcs11::new(path).map_err(|e| {
            CardError::new(SmartcardError::ModuleLoadFailed, format!("{path}: {e}"))
        })?;
        match ctx.initialize(CInitializeArgs::new(CInitializeFlags::OS_LOCKING_OK)) {
            Ok(()) | Err(CkError::Pkcs11(RvError::CryptokiAlreadyInitialized, _)) => {}
            Err(e) => {
                return Err(CardError::new(
                    SmartcardError::ModuleLoadFailed,
                    format!("{path}: {e}"),
                ));
            }
        }
        Ok(Self {
            path: path.to_owned(),
            ctx,
            sessions: Mutex::new(HashMap::new()),
            logged_in: Mutex::new(HashSet::new()),
        })
    }

    /// Runs `f` with the slot's session (opened on demand; a session the
    /// token dropped is reopened once).
    fn with_session<T>(
        &self,
        slot_id: u64,
        f: impl Fn(&Session) -> Result<T, CkError>,
    ) -> Result<T, CkError> {
        let mut sessions = lock(&self.sessions);
        for attempt in 0..2 {
            if let std::collections::hash_map::Entry::Vacant(entry) = sessions.entry(slot_id) {
                let s = self
                    .ctx
                    .open_ro_session(Slot::try_from(slot_id).map_err(|_| CkError::NotSupported)?)?;
                entry.insert(s);
                lock(&self.logged_in).remove(&slot_id);
            }
            let session = sessions.get(&slot_id).ok_or(CkError::NotSupported)?;
            match f(session) {
                Err(CkError::Pkcs11(RvError::SessionHandleInvalid | RvError::SessionClosed, _))
                    if attempt == 0 =>
                {
                    sessions.remove(&slot_id);
                }
                other => return other,
            }
        }
        Err(CkError::Pkcs11(
            RvError::SessionHandleInvalid,
            cryptoki::context::Function::OpenSession,
        ))
    }

    fn find(
        session: &Session,
        class: ObjectClass,
        id: &[u8],
    ) -> Result<Option<ObjectHandle>, CkError> {
        Ok(session
            .find_objects(&[Attribute::Class(class), Attribute::Id(id.to_vec())])?
            .into_iter()
            .next())
    }

    fn key(session: &Session, id: &[u8]) -> Result<ObjectHandle, CkError> {
        Self::find(session, ObjectClass::PRIVATE_KEY, id)?.ok_or(CkError::Pkcs11(
            RvError::KeyHandleInvalid,
            cryptoki::context::Function::FindObjects,
        ))
    }

    fn describe(session: &Session, handle: ObjectHandle, kind: SmartcardObjectKind) -> ObjectInfo {
        let attrs = session
            .get_attributes(
                handle,
                &[
                    AttributeType::Id,
                    AttributeType::Label,
                    AttributeType::KeyType,
                    AttributeType::Modulus,
                    AttributeType::PublicExponent,
                    AttributeType::EcParams,
                    AttributeType::EcPoint,
                    AttributeType::Sign,
                    AttributeType::Decrypt,
                    AttributeType::Derive,
                    AttributeType::Value,
                ],
            )
            .unwrap_or_default();
        let mut info = ObjectInfo {
            kind,
            id: Vec::new(),
            label: String::new(),
            public_key: None,
            can_sign: false,
            can_decrypt: false,
            certificate: None,
        };
        let (mut key_type, mut modulus, mut exponent, mut params, mut point) =
            (None, None, None, None, None);
        for attr in attrs {
            match attr {
                Attribute::Id(id) => info.id = id,
                Attribute::Label(label) => info.label = text(&label),
                Attribute::KeyType(t) => key_type = Some(t),
                Attribute::Modulus(m) => modulus = Some(m),
                Attribute::PublicExponent(e) => exponent = Some(e),
                Attribute::EcParams(p) => params = Some(p),
                Attribute::EcPoint(p) => point = Some(p),
                Attribute::Sign(b) => info.can_sign |= b,
                Attribute::Decrypt(b) | Attribute::Derive(b) => info.can_decrypt |= b,
                Attribute::Value(v) if kind == SmartcardObjectKind::Certificate => {
                    info.certificate = Some(v);
                }
                _ => {}
            }
        }
        info.public_key = match (key_type, modulus, exponent, params, point) {
            (Some(t), Some(modulus), Some(exponent), _, _) if t == KeyType::RSA => {
                Some(PublicKeyMaterial::Rsa { modulus, exponent })
            }
            (Some(t), _, _, Some(params), Some(point)) if t == KeyType::EC => {
                Some(PublicKeyMaterial::Ec {
                    params,
                    point: crate::unwrap_ec_point(&point),
                })
            }
            _ => None,
        };
        info
    }

    fn run<T>(
        &self,
        slot_id: u64,
        f: impl Fn(&Session) -> Result<T, CkError>,
    ) -> Result<Option<T>, CardError> {
        match self.with_session(slot_id, f) {
            Ok(out) => Ok(Some(out)),
            Err(err) if needs_login(&err) => {
                lock(&self.logged_in).remove(&slot_id);
                Ok(None)
            }
            Err(err) => Err(from_cryptoki(&err)),
        }
    }
}

impl Module for CryptokiModule {
    fn path(&self) -> &str {
        &self.path
    }

    fn tokens(&self) -> Result<Vec<TokenInfo>, CardError> {
        let slots = self
            .ctx
            .get_slots_with_token()
            .map_err(|e| from_cryptoki(&e))?;
        slots.into_iter().map(|s| self.token(s.id())).collect()
    }

    fn token(&self, slot_id: u64) -> Result<TokenInfo, CardError> {
        let info = self
            .ctx
            .get_token_info(slot(slot_id)?)
            .map_err(|e| from_cryptoki(&e))?;
        Ok(TokenInfo {
            slot_id,
            label: info.label().trim().to_owned(),
            manufacturer: info.manufacturer_id().trim().to_owned(),
            model: info.model().trim().to_owned(),
            serial: info.serial_number().trim().to_owned(),
            login_required: info.login_required(),
            pin_count_low: info.user_pin_count_low(),
            pin_final_try: info.user_pin_final_try(),
            pin_locked: info.user_pin_locked(),
        })
    }

    fn objects(&self, slot_id: u64) -> Result<Vec<ObjectInfo>, CardError> {
        self.with_session(slot_id, |session| {
            let mut out = Vec::new();
            for (class, kind) in [
                (ObjectClass::PRIVATE_KEY, SmartcardObjectKind::PrivateKey),
                (ObjectClass::CERTIFICATE, SmartcardObjectKind::Certificate),
                (ObjectClass::PUBLIC_KEY, SmartcardObjectKind::PublicKey),
            ] {
                for handle in session.find_objects(&[Attribute::Class(class)])? {
                    out.push(Self::describe(session, handle, kind));
                }
            }
            Ok(out)
        })
        .map_err(|e| from_cryptoki(&e))
    }

    fn is_logged_in(&self, slot_id: u64) -> bool {
        lock(&self.logged_in).contains(&slot_id)
    }

    fn login(&self, slot_id: u64, pin: &str) -> Result<(), CardError> {
        let pin = AuthPin::from(pin);
        match self.with_session(slot_id, |s| s.login(UserType::User, Some(&pin))) {
            Ok(()) | Err(CkError::Pkcs11(RvError::UserAlreadyLoggedIn, _)) => {
                lock(&self.logged_in).insert(slot_id);
                Ok(())
            }
            Err(err) => Err(from_cryptoki(&err)),
        }
    }

    fn logout(&self, slot_id: u64) {
        if lock(&self.logged_in).remove(&slot_id) {
            let _ = self.with_session(slot_id, Session::logout);
        }
    }

    fn sign(
        &self,
        slot_id: u64,
        id: &[u8],
        mechanism: SignMechanism,
        data: &[u8],
    ) -> Result<Option<Vec<u8>>, CardError> {
        self.run(slot_id, |session| {
            let key = Self::key(session, id)?;
            match mechanism {
                SignMechanism::RsaPkcs1 => session.sign(&Mechanism::RsaPkcs, key, data),
                SignMechanism::Ecdsa => session.sign(&Mechanism::Ecdsa, key, data),
                SignMechanism::RsaPss(hash) => {
                    let (hash_alg, mgf) = hash_mechanism(hash);
                    let params = PkcsPssParams {
                        hash_alg,
                        mgf,
                        s_len: cryptoki::types::Ulong::try_from(hash.len())?,
                    };
                    session.sign(&Mechanism::RsaPkcsPss(params), key, data)
                }
            }
        })
    }

    fn decrypt(
        &self,
        slot_id: u64,
        id: &[u8],
        mechanism: DecryptMechanism,
        data: &[u8],
    ) -> Result<Option<Zeroizing<Vec<u8>>>, CardError> {
        self.run(slot_id, |session| {
            let key = Self::key(session, id)?;
            let plain = match mechanism {
                DecryptMechanism::RsaPkcs1 => session.decrypt(&Mechanism::RsaPkcs, key, data)?,
                DecryptMechanism::RsaOaep(hash) => {
                    let (hash_alg, mgf) = hash_mechanism(hash);
                    let params = PkcsOaepParams::new(hash_alg, mgf, PkcsOaepSource::empty());
                    session.decrypt(&Mechanism::RsaPkcsOaep(params), key, data)?
                }
            };
            Ok(Zeroizing::new(plain))
        })
    }

    fn ecdh(
        &self,
        slot_id: u64,
        id: &[u8],
        peer_point: &[u8],
    ) -> Result<Option<Zeroizing<Vec<u8>>>, CardError> {
        self.run(slot_id, |session| {
            let key = Self::key(session, id)?;
            let params = Ecdh1DeriveParams::new(EcKdf::null(), peer_point);
            let template = [
                Attribute::Class(ObjectClass::SECRET_KEY),
                Attribute::KeyType(KeyType::GENERIC_SECRET),
                Attribute::Token(false),
                Attribute::Sensitive(false),
                Attribute::Extractable(true),
            ];
            let secret = session.derive_key(&Mechanism::Ecdh1Derive(params), key, &template)?;
            let value = session.get_attributes(secret, &[AttributeType::Value]);
            let _ = session.destroy_object(secret);
            let value = value?.into_iter().find_map(|a| match a {
                Attribute::Value(v) => Some(v),
                _ => None,
            });
            value.map(Zeroizing::new).ok_or(CkError::Pkcs11(
                RvError::AttributeSensitive,
                cryptoki::context::Function::GetAttributeValue,
            ))
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_and_broken_modules_are_refused() {
        let err = CryptokiModule::load(r"C:\does\not\exist\pkcs11.dll").unwrap_err();
        assert_eq!(err.kind, SmartcardError::ModuleNotFound);
        // A file that is not a DLL: libloading fails, nothing panics.
        let dir = tempfile::tempdir().unwrap();
        let fake = dir.path().join("fake-pkcs11.dll");
        std::fs::write(&fake, b"not a module").unwrap();
        let err = CryptokiModule::load(fake.to_str().unwrap()).unwrap_err();
        assert_eq!(err.kind, SmartcardError::ModuleLoadFailed);
    }

    #[test]
    fn labels_are_trimmed() {
        assert_eq!(text(b"OpenPGP card\0\0  "), "OpenPGP card");
    }
}
