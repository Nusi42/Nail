//! M4c smartcards and the vault recovery key requests (ADRs 0094, 0095,
//! 0099).
//!
//! * The PKCS#11 modules of the settings ([`SMARTCARD_SETTINGS_KEY`]) are
//!   loaded on first use ([`SmartcardCtl`]); `SetSmartcardSettings` logs
//!   out and loads them again. Tests and demos inject modules
//!   ([`crate::ServiceOptions::smartcard_modules`], e.g. the soft token).
//! * [`SmartcardCtl`] is the [`KeyResolver`] of both keyrings
//!   ([`Service::pgp`] / [`Service::smime`]), so signing and decrypting
//!   with a card key just works where the software keys do.
//! * PIN prompts: the backends ask [`CorePinSource`], which broadcasts
//!   `PinRequested` and waits (at most [`PIN_PROMPT_TIMEOUT_SECS`]) for
//!   `AnswerPinPrompt`; `PinPromptClosed` tells the other windows. PINs are
//!   only held until the login. Card work runs on the blocking pool (the
//!   PIN prompt blocks the calling thread).
//! * A poller notices inserted / removed tokens (`SmartcardsChanged`) and
//!   ends PIN-cache logins.

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use nail_api::engine::BoxFuture;
use nail_api::security::CryptoTechnology;
use nail_api::smartcard::{
    PIN_PROMPT_TIMEOUT_SECS, PinPrompt, PinPromptCloseReason, PinRequest, PinSource,
    SmartcardKeyRef, SmartcardSettings, SmartcardToken,
};
use nail_api::{ApiError, Event, LocalizedMessage, PinPromptId, Request, Response};
use nail_pkcs11::{BlockingPinSource, CardError, CardKey, KeyResolver, Module, Smartcards};
use tokio::sync::{broadcast, oneshot};

use crate::error::Result;
use crate::service::{Service, lock};

/// `app_settings` key of [`SmartcardSettings`].
pub const SMARTCARD_SETTINGS_KEY: &str = "security.smartcards";

/// How often the poller looks for inserted / removed tokens.
const POLL: Duration = Duration::from_secs(5);

/// Waiting PIN prompts: id → where the answer goes.
type Waiting = Arc<Mutex<HashMap<PinPromptId, oneshot::Sender<Option<String>>>>>;

/// The core's [`PinSource`]: PIN prompts over events.
#[derive(Debug, Clone)]
pub(crate) struct CorePinSource {
    events: broadcast::Sender<Event>,
    waiting: Waiting,
    next: Arc<AtomicU64>,
}

impl CorePinSource {
    fn close(&self, prompt_id: PinPromptId, reason: PinPromptCloseReason) {
        let _ = self
            .events
            .send(Event::PinPromptClosed { prompt_id, reason });
    }
}

impl PinSource for CorePinSource {
    fn request_pin(&self, request: PinRequest) -> BoxFuture<'_, Result<Option<String>, ApiError>> {
        Box::pin(async move {
            let n = self.next.fetch_add(1, Ordering::Relaxed) + 1;
            let prompt_id = PinPromptId::new(format!("pin-{n}"));
            let (tx, rx) = oneshot::channel();
            lock(&self.waiting).insert(prompt_id.clone(), tx);
            let expires = crate::engine::unix_now()
                + i64::try_from(PIN_PROMPT_TIMEOUT_SECS).unwrap_or(i64::MAX);
            let _ = self.events.send(Event::PinRequested {
                prompt: PinPrompt {
                    prompt_id: prompt_id.clone(),
                    token_label: request.token_label,
                    token_serial: request.token_serial,
                    purpose: request.purpose,
                    key_label: request.key_label,
                    attempts_left: request.attempts_left,
                    retry: request.retry,
                    expires_at: crate::rules::rfc3339(expires),
                },
            });
            match tokio::time::timeout(Duration::from_secs(PIN_PROMPT_TIMEOUT_SECS), rx).await {
                Ok(Ok(pin)) => Ok(pin),
                Ok(Err(_)) => {
                    self.close(prompt_id, PinPromptCloseReason::Cancelled);
                    Ok(None)
                }
                Err(_) => {
                    lock(&self.waiting).remove(&prompt_id);
                    self.close(prompt_id, PinPromptCloseReason::TimedOut);
                    Ok(None)
                }
            }
        })
    }
}

/// The smartcards of this run.
pub(crate) struct SmartcardCtl {
    cards: RwLock<Option<Smartcards>>,
    pins: CorePinSource,
    /// Modules given by the embedder (tests, demos) instead of the
    /// settings' DLLs.
    injected: Option<Vec<Arc<dyn Module>>>,
}

impl std::fmt::Debug for SmartcardCtl {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SmartcardCtl")
            .field("loaded", &self.loaded().is_some())
            .finish_non_exhaustive()
    }
}

impl SmartcardCtl {
    pub(crate) fn new(
        events: broadcast::Sender<Event>,
        injected: Option<Vec<Arc<dyn Module>>>,
    ) -> Self {
        Self {
            cards: RwLock::new(None),
            pins: CorePinSource {
                events,
                waiting: Arc::default(),
                next: Arc::default(),
            },
            injected,
        }
    }

    fn loaded(&self) -> Option<Smartcards> {
        self.cards
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
    }

    /// The modules, loaded from the settings on first use (blocking).
    pub(crate) fn cards(&self, store: &nail_store::Store) -> Smartcards {
        if let Some(cards) = self.loaded() {
            return cards;
        }
        let settings = checked_settings(
            store
                .setting(SMARTCARD_SETTINGS_KEY)
                .ok()
                .flatten()
                .unwrap_or_default(),
        );
        let prompter = Arc::new(BlockingPinSource::new(Arc::new(self.pins.clone())));
        let cards = match &self.injected {
            Some(modules) => {
                Smartcards::from_modules(modules.clone(), settings.pin_cache_minutes, prompter)
            }
            None => Smartcards::load(&settings, prompter),
        };
        let mut slot = self
            .cards
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        slot.get_or_insert(cards).clone()
    }

    /// Forgets the loaded modules (logging out first): the next use loads
    /// them with the new settings.
    pub(crate) fn reset(&self) {
        let old = self
            .cards
            .write()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .take();
        if let Some(cards) = old {
            cards.logout_all();
        }
    }

    /// Logs out of every card (lock / quit).
    pub(crate) fn logout_all(&self) {
        if let Some(cards) = self.loaded() {
            cards.logout_all();
        }
    }

    /// Answers a waiting prompt; `false` when it is gone.
    fn answer(&self, prompt_id: &PinPromptId, pin: Option<String>) -> bool {
        match lock(&self.pins.waiting).remove(prompt_id) {
            Some(tx) => {
                let cancelled = pin.is_none();
                let _ = tx.send(pin);
                self.pins.close(
                    prompt_id.clone(),
                    if cancelled {
                        PinPromptCloseReason::Cancelled
                    } else {
                        PinPromptCloseReason::Answered
                    },
                );
                true
            }
            None => false,
        }
    }
}

impl KeyResolver for SmartcardCtl {
    fn card_key(&self, key: &SmartcardKeyRef) -> Result<CardKey, CardError> {
        match self.loaded() {
            Some(cards) => cards.card_key(key),
            None => Err(CardError::new(
                nail_api::smartcard::SmartcardError::TokenNotPresent,
                "no smartcard modules are loaded",
            )),
        }
    }
}

/// The tokens with the keyring entries using their keys filled in
/// (blocking).
fn list(
    service: &Service,
) -> (
    Vec<SmartcardToken>,
    Vec<nail_api::protocol::SmartcardModuleProblem>,
) {
    let cards = service.inner.smartcard.cards(service.store());
    let (mut tokens, problems) = cards.list();
    let store = service.store();
    let pgp = service.pgp();
    for token in &mut tokens {
        for object in &mut token.objects {
            if let Some(cert) = &object.certificate
                && let Ok(Some(found)) = store.find_key(CryptoTechnology::Smime, &cert.sha256)
            {
                object.key_id = Some(found.id.clone());
            }
            let reference = SmartcardKeyRef {
                module: token.module.clone(),
                token_serial: token.serial.clone(),
                object_id: object.id.clone(),
            };
            if let Ok(info) = cards.describe_key(&reference)
                && let Some(material) = &info.public_key
                && let Ok(Some((key_id, fingerprint))) = pgp.fingerprint_for_card_key(material)
            {
                object.openpgp_fingerprint = Some(fingerprint);
                if object.key_id.is_none() {
                    object.key_id = Some(key_id);
                }
            }
        }
    }
    (tokens, problems)
}

/// Stored settings that fail [`SmartcardSettings::issues`] (only the
/// validated setter should store them; a damaged or foreign value must not
/// load a module through the DLL search order) load no modules.
fn checked_settings(settings: SmartcardSettings) -> SmartcardSettings {
    match settings.issues().first() {
        None => settings,
        Some(issue) => {
            tracing::warn!(issue = issue.as_str(), "stored smartcard settings refused");
            SmartcardSettings::default()
        }
    }
}

pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    match request {
        Request::GetSmartcardSettings => {
            let settings: SmartcardSettings = service
                .store()
                .call(|s| s.setting(SMARTCARD_SETTINGS_KEY))
                .await?
                .unwrap_or_default();
            Ok(Response::SmartcardSettings { settings })
        }
        Request::SetSmartcardSettings { settings } => {
            if let Some(issue) = settings.issues().first() {
                return Err(ApiError::invalid_request(format!(
                    "smartcard settings: {}",
                    issue.as_str()
                ))
                .with_localized(LocalizedMessage::new(issue.message_id()))
                .into());
            }
            let saved = settings.clone();
            service
                .store()
                .call(move |s| s.set_setting(SMARTCARD_SETTINGS_KEY, &saved))
                .await?;
            service.inner.smartcard.reset();
            service.broadcast(Event::SmartcardsChanged);
            Ok(Response::SmartcardSettings { settings })
        }
        Request::ListSmartcards => {
            let (tokens, problems) = service.blocking(|s| Ok(list(s))).await?;
            Ok(Response::Smartcards { tokens, problems })
        }
        Request::UseSmartcardKey { key, technology } => {
            let stored = service
                .blocking(move |s| {
                    s.inner.smartcard.cards(s.store());
                    Ok(match technology {
                        CryptoTechnology::Smime => {
                            let result = s.smime().use_smartcard_key(&key)?;
                            result.keys.into_iter().next().ok_or_else(|| {
                                ApiError::internal("the smartcard key was not stored")
                            })?
                        }
                        CryptoTechnology::OpenPgp => s.pgp().use_smartcard_key(&key)?,
                    })
                })
                .await?;
            service.broadcast(Event::KeysChanged);
            Ok(Response::Key {
                key: Box::new(stored),
            })
        }
        Request::AnswerPinPrompt { prompt_id, pin } => {
            if service.inner.smartcard.answer(&prompt_id, pin) {
                Ok(Response::Done)
            } else {
                Err(ApiError::not_found(format!("PIN prompt {prompt_id}"))
                    .with_localized(LocalizedMessage::new("smartcard-prompt-gone"))
                    .into())
            }
        }
        Request::CreateRecoveryKey { save_to } => service.create_recovery_key(save_to).await,
        Request::VerifyRecoveryKey { key } => service.verify_recovery_key(key).await,
        Request::RestoreVaultKey { key } => Ok(Response::VaultStatus {
            status: service.restore_vault_key(key).await?,
        }),
        Request::RemoveRecoveryKey => Ok(Response::VaultStatus {
            status: service.remove_recovery_key().await?,
        }),
        other => Err(ApiError::internal(format!(
            "{} routed to the smartcard module",
            crate::service::request_name(&other)
        ))
        .into()),
    }
}

/// Watches for inserted / removed tokens (only while modules are
/// configured) and ends PIN-cache logins.
pub(crate) async fn run_poller(service: Service) {
    let mut last: Option<Vec<(String, String)>> = None;
    loop {
        tokio::time::sleep(POLL).await;
        let configured = service
            .store()
            .call(|s| s.setting::<SmartcardSettings>(SMARTCARD_SETTINGS_KEY))
            .await
            .ok()
            .flatten()
            .is_some_and(|s| s.modules.iter().any(|m| m.enabled))
            || service.inner.smartcard.injected.is_some();
        if !configured {
            last = None;
            continue;
        }
        let seen = service
            .blocking(|s| {
                let cards = s.inner.smartcard.cards(s.store());
                cards.expire_logins();
                let (tokens, _) = cards.list();
                let mut ids: Vec<(String, String)> =
                    tokens.into_iter().map(|t| (t.module, t.serial)).collect();
                ids.sort();
                Ok(ids)
            })
            .await;
        if let Ok(seen) = seen {
            if last.as_ref().is_some_and(|l| l != &seen) {
                service.broadcast(Event::SmartcardsChanged);
            }
            last = Some(seen);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_pkcs11::testing::{SoftKey, SoftToken, test_rsa_key};
    use nail_pkcs11::{HashKind, SignMechanism, digest_info};

    const MODULE: &str = r"C:\test\soft.dll";

    fn ctl() -> (Arc<SmartcardCtl>, broadcast::Sender<Event>) {
        let (events, _) = broadcast::channel(32);
        let token = Arc::new(SoftToken::new(MODULE));
        token.add_token(1, "Test card", "S-1", "1234");
        token.add_key(
            1,
            &[1],
            "Signing key",
            SoftKey::Rsa(Box::new(test_rsa_key())),
            None,
            true,
        );
        let ctl = Arc::new(SmartcardCtl::new(
            events.clone(),
            Some(vec![token as Arc<dyn Module>]),
        ));
        ctl.cards(&nail_store::Store::open_in_memory().unwrap());
        (ctl, events)
    }

    #[test]
    fn stored_settings_with_relative_module_paths_load_nothing() {
        let module = |path: &str| nail_api::smartcard::Pkcs11Module {
            path: path.into(),
            name: None,
            enabled: true,
        };
        let bad = SmartcardSettings {
            modules: vec![module("evil.dll")],
            pin_cache_minutes: 5,
        };
        assert_eq!(checked_settings(bad), SmartcardSettings::default());
        let good = SmartcardSettings {
            modules: vec![module(r"C:\Windows\System32\card.dll")],
            pin_cache_minutes: 5,
        };
        assert_eq!(checked_settings(good.clone()), good);
    }

    fn key() -> SmartcardKeyRef {
        SmartcardKeyRef {
            module: MODULE.into(),
            token_serial: "S-1".into(),
            object_id: "01".into(),
        }
    }

    /// The PIN prompt goes out as an event, a wrong PIN asks again
    /// (`retry`), the answer closes the prompt for the other windows.
    #[tokio::test(flavor = "multi_thread")]
    async fn pin_prompts_go_through_events_and_retry() {
        let (ctl, events) = ctl();
        let mut rx = events.subscribe();
        let answering = {
            let ctl = Arc::clone(&ctl);
            tokio::spawn(async move {
                let mut prompts = Vec::new();
                let mut closed = Vec::new();
                while let Ok(event) = rx.recv().await {
                    match event {
                        Event::PinRequested { prompt } => {
                            let pin = if prompt.retry { "1234" } else { "0000" };
                            assert!(ctl.answer(&prompt.prompt_id, Some(pin.into())));
                            let done = prompt.retry;
                            prompts.push(prompt);
                            if done {
                                break;
                            }
                        }
                        Event::PinPromptClosed { reason, .. } => closed.push(reason),
                        _ => {}
                    }
                }
                (prompts, closed)
            })
        };
        let signature = tokio::task::spawn_blocking({
            let ctl = Arc::clone(&ctl);
            move || {
                let card = ctl.card_key(&key()).unwrap();
                let input = digest_info(HashKind::Sha256, &[7u8; 32]).unwrap();
                card.sign(SignMechanism::RsaPkcs1, &input)
            }
        })
        .await
        .unwrap()
        .unwrap();
        assert!(!signature.is_empty());
        let (prompts, closed) = answering.await.unwrap();
        assert_eq!(prompts.len(), 2);
        assert_eq!(prompts[0].token_label, "Test card");
        assert!(!prompts[0].retry && prompts[1].retry);
        assert!(closed.iter().all(|r| *r == PinPromptCloseReason::Answered));
        // A prompt that is gone cannot be answered.
        assert!(!ctl.answer(&PinPromptId::new("pin-99"), None));
    }

    /// Cancelling the prompt stops the operation.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_cancelled_prompt_stops_the_operation() {
        let (ctl, events) = ctl();
        let mut rx = events.subscribe();
        {
            let ctl = Arc::clone(&ctl);
            tokio::spawn(async move {
                while let Ok(event) = rx.recv().await {
                    if let Event::PinRequested { prompt } = event {
                        ctl.answer(&prompt.prompt_id, None);
                        break;
                    }
                }
            });
        }
        let result = tokio::task::spawn_blocking({
            let ctl = Arc::clone(&ctl);
            move || {
                let card = ctl.card_key(&key()).unwrap();
                let input = digest_info(HashKind::Sha256, &[7u8; 32]).unwrap();
                card.sign(SignMechanism::RsaPkcs1, &input)
            }
        })
        .await
        .unwrap();
        assert!(result.is_err());
        // After a reset nothing is loaded until the next use.
        ctl.reset();
        assert!(ctl.loaded().is_none());
        assert!(ctl.card_key(&key()).is_err());
    }
}
