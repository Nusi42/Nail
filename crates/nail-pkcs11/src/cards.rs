//! [`Smartcards`]: the configured modules, the token listing and
//! [`CardKey`]s with the PIN flow and the PIN cache.

use std::collections::HashMap;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use nail_api::protocol::SmartcardModuleProblem;
use nail_api::smartcard::{
    PinPurpose, PinRequest, SmartcardError, SmartcardKeyRef, SmartcardKeyUsage, SmartcardObject,
    SmartcardObjectKind, SmartcardSettings, SmartcardToken,
};
use zeroize::Zeroizing;

use crate::cryptoki_module::CryptokiModule;
use crate::error::CardError;
use crate::module::{
    DecryptMechanism, Module, ObjectInfo, PublicKeyMaterial, SignMechanism, TokenInfo,
};
use crate::pin::PinPrompter;

/// How many PIN prompts one operation may show before giving up (the card
/// locks itself after its own retry limit anyway).
const MAX_PIN_PROMPTS: usize = 5;

/// Resolves the keyring's smartcard references to usable card keys
/// (`nail-pgp` / `nail-smime` take one).
pub trait KeyResolver: Send + Sync + std::fmt::Debug {
    /// The key a [`SmartcardKeyRef`] names, on an inserted token.
    fn card_key(&self, key: &SmartcardKeyRef) -> Result<CardKey, CardError>;
}

#[derive(Debug)]
struct Shared {
    modules: Vec<Arc<dyn Module>>,
    problems: Vec<SmartcardModuleProblem>,
    prompter: Arc<dyn PinPrompter>,
    pin_cache: Duration,
    /// Logged-in slots and until when the login may be used.
    logins: Mutex<HashMap<(usize, u64), Instant>>,
}

/// The smartcard modules of the settings, with the PIN prompter.
#[derive(Debug, Clone)]
pub struct Smartcards {
    shared: Arc<Shared>,
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

impl Smartcards {
    /// Loads the enabled modules of `settings`. Modules that fail to load
    /// are reported by [`Smartcards::problems`], the rest work.
    pub fn load(settings: &SmartcardSettings, prompter: Arc<dyn PinPrompter>) -> Self {
        let mut modules: Vec<Arc<dyn Module>> = Vec::new();
        let mut problems = Vec::new();
        for module in settings.modules.iter().filter(|m| m.enabled) {
            match CryptokiModule::load(&module.path) {
                Ok(loaded) => modules.push(Arc::new(loaded)),
                Err(err) => {
                    tracing::warn!(path = %module.path, %err, "PKCS#11 module not usable");
                    problems.push(SmartcardModuleProblem {
                        module: module.path.clone(),
                        error: err.kind,
                        detail: err.detail,
                    });
                }
            }
        }
        Self::build(modules, problems, settings.pin_cache_minutes, prompter)
    }

    /// Over already loaded modules (tests use the soft token).
    pub fn from_modules(
        modules: Vec<Arc<dyn Module>>,
        pin_cache_minutes: u32,
        prompter: Arc<dyn PinPrompter>,
    ) -> Self {
        Self::build(modules, Vec::new(), pin_cache_minutes, prompter)
    }

    fn build(
        modules: Vec<Arc<dyn Module>>,
        problems: Vec<SmartcardModuleProblem>,
        pin_cache_minutes: u32,
        prompter: Arc<dyn PinPrompter>,
    ) -> Self {
        Self {
            shared: Arc::new(Shared {
                modules,
                problems,
                prompter,
                pin_cache: Duration::from_secs(u64::from(pin_cache_minutes) * 60),
                logins: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Modules that could not be loaded.
    pub fn problems(&self) -> &[SmartcardModuleProblem] {
        &self.shared.problems
    }

    /// Every present token of every module with its objects (the answer of
    /// `ListSmartcards`), plus the load problems and modules that failed
    /// while listing. `key_id` / `openpgp_fingerprint` are left for the
    /// caller (it knows the keyring).
    pub fn list(&self) -> (Vec<SmartcardToken>, Vec<SmartcardModuleProblem>) {
        let mut tokens = Vec::new();
        let mut problems = self.shared.problems.clone();
        for module in &self.shared.modules {
            let found = match module.tokens() {
                Ok(found) => found,
                Err(err) => {
                    problems.push(problem(module.as_ref(), err));
                    continue;
                }
            };
            for token in found {
                let objects = match module.objects(token.slot_id) {
                    Ok(objects) => objects,
                    Err(err) => {
                        problems.push(problem(module.as_ref(), err));
                        continue;
                    }
                };
                tokens.push(SmartcardToken {
                    module: module.path().to_owned(),
                    slot_id: token.slot_id,
                    label: token.label.clone(),
                    manufacturer: token.manufacturer.clone(),
                    model: token.model.clone(),
                    serial: token.serial.clone(),
                    login_required: token.login_required,
                    pin_count_low: token.pin_count_low,
                    pin_locked: token.pin_locked,
                    objects: contract_objects(&objects),
                });
            }
        }
        (tokens, problems)
    }

    fn locate(&self, key: &SmartcardKeyRef) -> Result<(usize, TokenInfo, ObjectInfo), CardError> {
        let index = self
            .shared
            .modules
            .iter()
            .position(|m| m.path().eq_ignore_ascii_case(&key.module))
            .ok_or_else(|| {
                CardError::new(
                    SmartcardError::ModuleNotFound,
                    format!("the module {} is not configured", key.module),
                )
            })?;
        let module = &self.shared.modules[index];
        let token = module
            .tokens()?
            .into_iter()
            .find(|t| t.serial == key.token_serial)
            .ok_or_else(|| {
                CardError::new(
                    SmartcardError::TokenNotPresent,
                    format!("the token {} is not inserted", key.token_serial),
                )
            })?;
        let id = hex::decode(&key.object_id)
            .map_err(|_| CardError::new(SmartcardError::KeyNotFound, "the key id is not hex"))?;
        let objects = module.objects(token.slot_id)?;
        let private = objects
            .iter()
            .find(|o| o.kind == SmartcardObjectKind::PrivateKey && o.id == id)
            .ok_or_else(|| {
                CardError::new(
                    SmartcardError::KeyNotFound,
                    format!("no private key {} on the token", key.object_id),
                )
            })?;
        let mut object = private.clone();
        if object.public_key.is_none() {
            object.public_key = paired_public_key(&objects, &id);
        }
        if object.certificate.is_none() {
            object.certificate = objects
                .iter()
                .find(|o| o.kind == SmartcardObjectKind::Certificate && o.id == id)
                .and_then(|o| o.certificate.clone());
        }
        Ok((index, token, object))
    }

    /// The private key a reference names, with its certificate and public
    /// key when the token has them (for `UseSmartcardKey`).
    pub fn describe_key(&self, key: &SmartcardKeyRef) -> Result<ObjectInfo, CardError> {
        self.locate(key).map(|(_, _, object)| object)
    }

    /// Logs out every slot whose PIN-cache time is over (the core may call
    /// this periodically; card operations check it too).
    pub fn expire_logins(&self) {
        let now = Instant::now();
        let expired: Vec<(usize, u64)> = lock(&self.shared.logins)
            .iter()
            .filter(|(_, until)| **until <= now)
            .map(|(k, _)| *k)
            .collect();
        for (index, slot) in expired {
            self.logout(index, slot);
        }
    }

    /// Logs out of every card (lock / quit).
    pub fn logout_all(&self) {
        let all: Vec<(usize, u64)> = lock(&self.shared.logins).keys().copied().collect();
        for (index, slot) in all {
            self.logout(index, slot);
        }
    }

    fn logout(&self, index: usize, slot: u64) {
        lock(&self.shared.logins).remove(&(index, slot));
        if let Some(module) = self.shared.modules.get(index) {
            module.logout(slot);
        }
    }
}

impl KeyResolver for Smartcards {
    fn card_key(&self, key: &SmartcardKeyRef) -> Result<CardKey, CardError> {
        let (module, token, object) = self.locate(key)?;
        Ok(CardKey {
            cards: self.clone(),
            module,
            key: key.clone(),
            token_label: token.label,
            object,
        })
    }
}

fn problem(module: &dyn Module, err: CardError) -> SmartcardModuleProblem {
    SmartcardModuleProblem {
        module: module.path().to_owned(),
        error: err.kind,
        detail: err.detail,
    }
}

fn paired_public_key(objects: &[ObjectInfo], id: &[u8]) -> Option<PublicKeyMaterial> {
    objects.iter().filter(|o| o.id == id).find_map(|o| {
        o.public_key.clone().or_else(|| {
            o.certificate.as_deref().and_then(|der| {
                use der::Decode;
                let cert = x509_cert::Certificate::from_der(der).ok()?;
                crate::public_key_of_spki(&cert.tbs_certificate.subject_public_key_info)
            })
        })
    })
}

/// The contract's view of a token's objects.
fn contract_objects(objects: &[ObjectInfo]) -> Vec<SmartcardObject> {
    objects
        .iter()
        .map(|o| {
            let public = o
                .public_key
                .clone()
                .or_else(|| paired_public_key(objects, &o.id));
            let mut usages = Vec::new();
            if o.kind == SmartcardObjectKind::PrivateKey {
                if o.can_sign {
                    usages.push(SmartcardKeyUsage::Sign);
                }
                if o.can_decrypt {
                    usages.push(SmartcardKeyUsage::Decrypt);
                }
            }
            SmartcardObject {
                kind: o.kind,
                id: hex::encode(&o.id),
                label: o.label.clone(),
                algorithm: public.as_ref().map(PublicKeyMaterial::algorithm_name),
                usages,
                certificate: o.certificate.as_deref().and_then(crate::certificate_facts),
                openpgp_fingerprint: None,
                key_id: None,
            }
        })
        .collect()
}

/// A private key on a card. Every operation finds the token by serial (it
/// may have moved to another reader), asks for the PIN when the card
/// wants a login, and logs out again when the PIN cache is off.
#[derive(Debug, Clone)]
pub struct CardKey {
    cards: Smartcards,
    module: usize,
    key: SmartcardKeyRef,
    token_label: String,
    object: ObjectInfo,
}

impl CardKey {
    /// The reference.
    pub fn key_ref(&self) -> &SmartcardKeyRef {
        &self.key
    }

    /// The label of the token the key is on.
    pub fn token_label(&self) -> &str {
        &self.token_label
    }

    /// The key's label on the card.
    pub fn label(&self) -> &str {
        &self.object.label
    }

    /// The public key, when the token shows it (or a paired public key /
    /// certificate does).
    pub fn public_key(&self) -> Option<&PublicKeyMaterial> {
        self.object.public_key.as_ref()
    }

    /// The certificate stored with the key, if any.
    pub fn certificate(&self) -> Option<&[u8]> {
        self.object.certificate.as_deref()
    }

    fn module(&self) -> &Arc<dyn Module> {
        &self.cards.shared.modules[self.module]
    }

    fn slot(&self) -> Result<TokenInfo, CardError> {
        self.module()
            .tokens()?
            .into_iter()
            .find(|t| t.serial == self.key.token_serial)
            .ok_or_else(|| {
                CardError::new(
                    SmartcardError::TokenNotPresent,
                    format!("the token {} is not inserted", self.key.token_serial),
                )
            })
    }

    fn id(&self) -> Vec<u8> {
        self.object.id.clone()
    }

    /// Runs `op`; when the card wants a login, asks for the PIN (again
    /// after a wrong one) and retries.
    fn with_login<T>(
        &self,
        purpose: PinPurpose,
        op: impl Fn(&dyn Module, u64) -> Result<Option<T>, CardError>,
    ) -> Result<T, CardError> {
        let shared = &self.cards.shared;
        let token = self.slot()?;
        let slot = token.slot_id;
        let login_key = (self.module, slot);
        // A cached login past its time is dropped first.
        let expired = lock(&shared.logins)
            .get(&login_key)
            .is_some_and(|until| *until <= Instant::now());
        if expired {
            self.cards.logout(self.module, slot);
        }
        let module = self.module().as_ref();
        if token.pin_locked {
            return Err(CardError::new(
                SmartcardError::PinLocked,
                "the user PIN is locked",
            ));
        }
        let out = match op(module, slot)? {
            Some(out) => out,
            None => {
                self.login(module, &token, purpose)?;
                op(module, slot)?.ok_or_else(|| {
                    CardError::new(
                        SmartcardError::DeviceError,
                        "the card still wants a login after the PIN was accepted",
                    )
                })?
            }
        };
        if module.is_logged_in(slot) {
            if shared.pin_cache.is_zero() {
                self.cards.logout(self.module, slot);
            } else {
                lock(&shared.logins)
                    .entry(login_key)
                    .or_insert_with(|| Instant::now() + shared.pin_cache);
            }
        }
        Ok(out)
    }

    fn login(
        &self,
        module: &dyn Module,
        token: &TokenInfo,
        purpose: PinPurpose,
    ) -> Result<(), CardError> {
        let mut retry = false;
        for _ in 0..MAX_PIN_PROMPTS {
            let fresh = module
                .token(token.slot_id)
                .unwrap_or_else(|_| token.clone());
            if fresh.pin_locked {
                return Err(CardError::new(
                    SmartcardError::PinLocked,
                    "the user PIN is locked",
                ));
            }
            let request = PinRequest {
                token_label: fresh.label.clone(),
                token_serial: fresh.serial.clone(),
                purpose,
                key_label: Some(self.object.label.clone()).filter(|l| !l.is_empty()),
                attempts_left: fresh.pin_final_try.then_some(1),
                retry,
            };
            let pin: Option<Zeroizing<String>> = self
                .cards
                .shared
                .prompter
                .request_pin(request)
                .map_err(|e| CardError::new(SmartcardError::PinCancelled, e.message))?;
            let Some(pin) = pin else {
                return Err(CardError::new(
                    SmartcardError::PinCancelled,
                    "no PIN was entered",
                ));
            };
            match module.login(token.slot_id, &pin) {
                Ok(()) => return Ok(()),
                Err(err) if err.kind == SmartcardError::PinIncorrect => retry = true,
                Err(err) => return Err(err),
            }
        }
        Err(CardError::new(
            SmartcardError::PinIncorrect,
            "too many wrong PINs in a row",
        ))
    }

    /// Signs `data` (a digest, or a `DigestInfo` for
    /// [`SignMechanism::RsaPkcs1`]).
    pub fn sign(&self, mechanism: SignMechanism, data: &[u8]) -> Result<Vec<u8>, CardError> {
        let id = self.id();
        self.with_login(PinPurpose::Sign, |module, slot| {
            module.sign(slot, &id, mechanism, data)
        })
    }

    /// Decrypts a key-transport ciphertext.
    pub fn decrypt(
        &self,
        mechanism: DecryptMechanism,
        data: &[u8],
    ) -> Result<Zeroizing<Vec<u8>>, CardError> {
        let id = self.id();
        self.with_login(PinPurpose::Decrypt, |module, slot| {
            module.decrypt(slot, &id, mechanism, data)
        })
    }

    /// ECDH with a peer's SEC1 point: the raw shared secret.
    pub fn ecdh(&self, peer_point: &[u8]) -> Result<Zeroizing<Vec<u8>>, CardError> {
        let id = self.id();
        self.with_login(PinPurpose::Decrypt, |module, slot| {
            module.ecdh(slot, &id, peer_point)
        })
    }
}
