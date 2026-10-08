//! Requests of wave M2a (ADRs 0029, 0034): keys and certificates, recipient
//! key status, Autocrypt setup messages, the security settings and
//! encryption at rest.
//!
//! [`Service::dispatch`](crate::service::Service) routes exactly these
//! requests here (one match arm). OpenPGP goes to `nail-pgp`, S/MIME to
//! `nail-smime` (both over the store's one keyring), encryption at rest to
//! `nail-vault` ([`crate::vault`]). Every change to the keyring is
//! announced with [`Event::KeysChanged`].

use base64::Engine as _;
use nail_api::security::{
    AutocryptRecommendation, CryptoAvailability, CryptoKey, CryptoTechnology, KeyEncoding,
    KeyImportResult, KeyLookupPolicy, KeyLookupState, KeySource, RecipientKeyState,
    RecipientKeysReport, SecuritySettings, SendCrypto,
};
use nail_api::{
    Address, ApiError, AutomaticMail, Draft, DraftAttachment, ErrorCode, Event, KeyId, Request,
    Response,
};
use nail_search::IndexPolicy;
use nail_store::SECURITY_SETTINGS_KEY;

use crate::engine::unix_now;
use crate::error::Result;
use crate::service::Service;

/// Handles one of this module's requests.
pub(crate) async fn handle(service: &Service, request: Request) -> Result<Response> {
    match request {
        Request::GetSecuritySettings => Ok(Response::SecuritySettings {
            settings: security_settings(service).await?,
        }),
        Request::UpdateSecuritySettings { settings } => {
            let previous = security_settings(service).await?;
            let settings = update_security_settings(service, settings).await?;
            service.broadcast(Event::SecuritySettingsChanged);
            reindex_for_policy_change(service, &previous, &settings);
            Ok(Response::SecuritySettings { settings })
        }
        Request::GetVaultStatus => Ok(Response::VaultStatus {
            status: service.inner.vault.status()?,
        }),
        Request::SuggestAuthservId { account_id } => {
            // An unknown account is an error, not "no suggestion".
            service.store().get_account(&account_id)?;
            let lookup = account_id.clone();
            let authserv_id = service
                .blocking(move |s| s.suggest_authserv_id(&lookup))
                .await?;
            Ok(Response::AuthservIdSuggestion {
                account_id,
                authserv_id,
            })
        }
        Request::SetVaultEncryption { enabled } => Ok(Response::VaultStatus {
            status: service.set_vault_encryption(enabled).await?,
        }),
        Request::ListKeys { filter } => {
            let keys = service
                .blocking(move |s| {
                    let mut keys = Vec::new();
                    if filter.technology != Some(CryptoTechnology::Smime) {
                        keys.extend(s.pgp().list(&filter)?);
                    }
                    if filter.technology != Some(CryptoTechnology::OpenPgp) {
                        keys.extend(s.smime().list(&filter)?);
                    }
                    Ok(sort_keys(keys))
                })
                .await?;
            Ok(Response::Keys { keys })
        }
        Request::GetKey { key_id } => {
            let key = service.blocking(move |s| get_key(s, &key_id)).await?;
            Ok(Response::Key { key: Box::new(key) })
        }
        Request::ImportKeys { data, encoding } => {
            let result = service
                .blocking(move |s| import_keys(s, &data, encoding))
                .await?;
            keys_changed(service, &result);
            Ok(Response::KeysImported {
                result: Box::new(result),
            })
        }
        Request::ImportPkcs12 {
            data,
            passphrase,
            protection,
        } => {
            let bytes = decode_base64(&data)?;
            let result = service
                .blocking(move |s| Ok(s.smime().import_pkcs12(&bytes, &passphrase, &protection)?))
                .await?;
            keys_changed(service, &result);
            Ok(Response::KeysImported {
                result: Box::new(result),
            })
        }
        Request::ExportPublicKey { key_id, encoding } => {
            service
                .blocking(move |s| {
                    Ok(match technology_of(s, &key_id)? {
                        CryptoTechnology::OpenPgp => {
                            let e = s.pgp().export_public(&key_id, encoding)?;
                            Response::KeyExported {
                                filename: e.filename,
                                mime: e.mime,
                                encoding: e.encoding,
                                data: e.data,
                            }
                        }
                        CryptoTechnology::Smime => {
                            let e = s.smime().export_public(&key_id, encoding)?;
                            Response::KeyExported {
                                filename: e.filename,
                                mime: e.mime,
                                encoding: e.encoding,
                                data: e.data,
                            }
                        }
                    })
                })
                .await
        }
        Request::GenerateKey {
            identity_id,
            algorithm,
            expires_days,
            protection,
        } => {
            let (_, identity) = service.identity(&identity_id)?;
            let key = service
                .blocking(move |s| {
                    Ok(s.pgp().generate(
                        identity.name.as_deref().unwrap_or_default(),
                        &identity.email,
                        algorithm,
                        expires_days,
                        &protection,
                    )?)
                })
                .await?;
            service.broadcast(Event::KeysChanged);
            Ok(Response::KeyGenerated { key: Box::new(key) })
        }
        Request::DeleteKey {
            key_id,
            delete_secret,
        } => {
            let id = key_id.clone();
            let deleted = service
                .blocking(move |s| {
                    Ok(match technology_of(s, &id)? {
                        CryptoTechnology::OpenPgp => s.pgp().delete(&id, delete_secret)?,
                        CryptoTechnology::Smime => s.smime().delete(&id, delete_secret)?,
                    })
                })
                .await?;
            if !deleted {
                return Err(ApiError::not_found(format!("key {key_id}")).into());
            }
            service.broadcast(Event::KeysChanged);
            Ok(Response::Done)
        }
        Request::SetKeyTrust { key_id, decision } => {
            let key = service
                .blocking(move |s| {
                    Ok(match technology_of(s, &key_id)? {
                        CryptoTechnology::OpenPgp => s.pgp().set_trust(&key_id, decision)?,
                        CryptoTechnology::Smime => {
                            let stored = s.store().get_key(&key_id)?;
                            s.store().set_trust_decision(
                                CryptoTechnology::Smime,
                                &stored.data.fingerprint,
                                decision,
                            )?;
                            s.smime().get(&key_id)?
                        }
                    })
                })
                .await?;
            service.broadcast(Event::KeysChanged);
            Ok(Response::KeySaved { key: Box::new(key) })
        }
        Request::UnlockKey {
            key_id,
            passphrase,
            remember_minutes,
        } => {
            service
                .blocking(move |s| {
                    match technology_of(s, &key_id)? {
                        CryptoTechnology::OpenPgp => {
                            s.pgp().unlock(&key_id, &passphrase, remember_minutes)?;
                        }
                        CryptoTechnology::Smime => {
                            s.smime().unlock(&key_id, &passphrase, remember_minutes)?;
                        }
                    }
                    Ok(())
                })
                .await?;
            // Messages that were locked can be read now.
            service.broadcast(Event::KeysChanged);
            Ok(Response::Done)
        }
        Request::LookupKeys { email, sources } => {
            let settings = security_settings(service).await?;
            if settings.key_lookup == KeyLookupPolicy::Off {
                return Err(ApiError::unsupported("online key lookup is turned off").into());
            }
            let email = email.trim().to_ascii_lowercase();
            if !email.contains('@') {
                // Nail's own text, never English as a `detail` (M2d review).
                let problem = nail_pgp::Problem::NotAnAddress { address: email };
                return Err(ApiError::from(nail_pgp::Error::from(problem)).into());
            }
            let sources = if sources.is_empty() {
                settings.lookup_sources
            } else {
                sources
            };
            let found = nail_pgp::discovery::lookup(
                service.inner.crypto.http.as_ref(),
                &email,
                &sources,
                unix_now(),
            )
            .await?;
            let failures = found.failures;
            let mut candidates = found.candidates;
            let candidates = service
                .blocking(move |s| {
                    nail_pgp::discovery::mark_known(&s.pgp(), &mut candidates)?;
                    Ok(candidates)
                })
                .await?;
            Ok(Response::KeyLookup {
                email,
                candidates,
                failures,
            })
        }
        Request::ListWindowsCertificates { store } => {
            let certificates = service
                .blocking(move |s| Ok(s.smime().windows_certificates(store)?))
                .await?;
            Ok(Response::WindowsCertificates { certificates })
        }
        Request::ImportWindowsCertificate { store, thumbprint } => {
            let result = service
                .blocking(move |s| Ok(s.smime().import_windows_certificate(store, &thumbprint)?))
                .await?;
            keys_changed(service, &result);
            Ok(Response::KeysImported {
                result: Box::new(result),
            })
        }
        Request::CheckRecipientKeys {
            identity_id,
            recipients,
            lookup,
        } => Ok(Response::RecipientKeys {
            report: Box::new(
                check_recipient_keys(service, &identity_id, recipients, lookup).await?,
            ),
        }),
        Request::SendAutocryptSetup { identity_id } => {
            send_autocrypt_setup(service, &identity_id).await
        }
        Request::ImportAutocryptSetup {
            message_id,
            passcode,
        } => {
            let result = service
                .blocking(move |s| {
                    let stored = s.store().get_message(&message_id)?;
                    let hash = stored.blob_hash.ok_or_else(|| {
                        ApiError::new(
                            ErrorCode::Unavailable,
                            format!("the body of message {message_id} is not downloaded yet"),
                        )
                    })?;
                    let raw = s.blobs().get(&hash)?;
                    let (result, _prefer) =
                        nail_pgp::autocrypt::import_setup(&s.pgp(), &raw, &passcode)?;
                    Ok(result)
                })
                .await?;
            keys_changed(service, &result);
            Ok(Response::KeysImported {
                result: Box::new(result),
            })
        }
        other => Err(ApiError::internal(format!(
            "{} is not a security request",
            other.wire_type()
        ))
        .into()),
    }
}

fn keys_changed(service: &Service, result: &KeyImportResult) {
    if result.created > 0 || result.updated > 0 {
        service.broadcast(Event::KeysChanged);
    }
}

/// Own keys first, then by primary address (stable within).
fn sort_keys(mut keys: Vec<CryptoKey>) -> Vec<CryptoKey> {
    keys.sort_by_key(|k| (!k.has_secret, k.emails.first().cloned().unwrap_or_default()));
    keys
}

fn technology_of(service: &Service, key_id: &KeyId) -> Result<CryptoTechnology> {
    Ok(service.store().get_key(key_id)?.data.technology)
}

fn get_key(service: &Service, key_id: &KeyId) -> Result<CryptoKey> {
    Ok(match technology_of(service, key_id)? {
        CryptoTechnology::OpenPgp => service.pgp().get(key_id)?,
        CryptoTechnology::Smime => service.smime().get(key_id)?,
    })
}

fn decode_base64(data: &str) -> Result<Vec<u8>> {
    let compact: String = data.chars().filter(|c| !c.is_whitespace()).collect();
    base64::engine::general_purpose::STANDARD
        .decode(compact)
        .map_err(|_| ApiError::invalid_request("data is not valid base64").into())
}

/// Whether imported data is OpenPGP: armored, or a binary packet (the
/// first byte of every OpenPGP packet has bit 7 set; DER starts with 0x30).
fn is_openpgp(bytes: &[u8]) -> bool {
    let armored = bytes
        .windows(b"-----BEGIN PGP".len())
        .any(|w| w == b"-----BEGIN PGP");
    armored || bytes.first().is_some_and(|b| b & 0x80 != 0)
}

/// `ImportKeys`: OpenPGP keys to `nail-pgp`, certificates to `nail-smime`.
fn import_keys(service: &Service, data: &str, encoding: KeyEncoding) -> Result<KeyImportResult> {
    let bytes = match encoding {
        KeyEncoding::Text => data.as_bytes().to_vec(),
        KeyEncoding::Base64 => decode_base64(data)?,
    };
    if is_openpgp(&bytes) {
        return Ok(service.pgp().import(data, encoding)?);
    }
    if nail_smime::Cert::parse_many(&bytes).is_ok_and(|c| !c.is_empty()) {
        return Ok(service
            .smime()
            .import_certificates(&bytes, KeySource::Imported)?);
    }
    Err(ApiError::new(ErrorCode::Crypto, "no OpenPGP key or certificate found").into())
}

/// `CheckRecipientKeys`.
async fn check_recipient_keys(
    service: &Service,
    identity_id: &nail_api::IdentityId,
    recipients: Vec<String>,
    lookup: bool,
) -> Result<RecipientKeysReport> {
    let (account, identity) = service.identity(identity_id)?;
    let settings = security_settings(service).await?;
    let crypto = identity.crypto.clone().unwrap_or_default();
    let mut emails: Vec<String> = Vec::new();
    for r in recipients {
        let r = r.trim().to_ascii_lowercase();
        if !r.is_empty() && !emails.contains(&r) {
            emails.push(r);
        }
    }
    let lookup_identity = identity.clone();
    let lookup_emails = emails.clone();
    let mut report = service
        .blocking(move |s| {
            let identity = lookup_identity;
            let pgp = s.pgp();
            let smime = s.smime();
            let mut statuses = Vec::new();
            for email in &lookup_emails {
                let mut status = nail_pgp::autocrypt::recipient_status(&pgp, &account.id, email)?;
                status.smime = smime.recipient_key(email)?;
                if !crypto.autocrypt {
                    status.autocrypt = None;
                }
                statuses.push(status);
            }
            let own = crate::crypto::own_keys(s, &identity)?;
            let usable =
                |state: Option<RecipientKeyState>| state == Some(RecipientKeyState::Usable);
            let openpgp = CryptoAvailability {
                own_key: own.openpgp,
                all_recipients: !statuses.is_empty()
                    && statuses
                        .iter()
                        .all(|r| usable(r.openpgp.as_ref().map(|k| k.state))),
            };
            let smime_availability = CryptoAvailability {
                own_key: own.smime,
                all_recipients: !statuses.is_empty()
                    && statuses
                        .iter()
                        .all(|r| usable(r.smime.as_ref().map(|k| k.state))),
            };
            let autocrypt = if crypto.autocrypt {
                nail_pgp::autocrypt::recommend(
                    &pgp,
                    &account.id,
                    &identity.email,
                    crypto.prefer_encrypt,
                    own.openpgp,
                    &lookup_emails,
                    false,
                )?
            } else {
                AutocryptRecommendation::Disable
            };
            let can_encrypt = (openpgp.own_key && openpgp.all_recipients)
                || (smime_availability.own_key && smime_availability.all_recipients);
            Ok(RecipientKeysReport {
                recipients: statuses,
                openpgp,
                smime: smime_availability,
                autocrypt,
                suggested: SendCrypto {
                    sign: crypto.sign,
                    encrypt: can_encrypt
                        && (crypto.encrypt || autocrypt == AutocryptRecommendation::Encrypt),
                    technology: None,
                    protect_headers: None,
                },
            })
        })
        .await?;
    if lookup && settings.key_lookup != KeyLookupPolicy::Off {
        let now = unix_now();
        for status in &mut report.recipients {
            if status
                .openpgp
                .as_ref()
                .is_some_and(|k| k.state == RecipientKeyState::Usable)
            {
                continue;
            }
            status.lookup = match nail_pgp::discovery::lookup(
                service.inner.crypto.http.as_ref(),
                &status.email,
                &settings.lookup_sources,
                now,
            )
            .await
            {
                Ok(found) if !found.candidates.is_empty() => KeyLookupState::Found,
                Ok(found) if !found.failures.is_empty() => KeyLookupState::Failed,
                Ok(_) => KeyLookupState::NotFound,
                Err(_) => KeyLookupState::Failed,
            };
        }
    }
    Ok(report)
}

/// `SendAutocryptSetup`: the setup message goes through the outbox to the
/// identity itself; the passcode is returned once.
async fn send_autocrypt_setup(
    service: &Service,
    identity_id: &nail_api::IdentityId,
) -> Result<Response> {
    let (_, identity) = service.identity(identity_id)?;
    let locale = service.inner.locale.clone();
    let setup_identity = identity.clone();
    let (raw, passcode) = service
        .blocking(move |s| {
            let identity = setup_identity;
            let crypto = identity.crypto.clone().unwrap_or_default();
            let pgp = s.pgp();
            let own = pgp.own_key_for(&identity.email, crypto.openpgp_key.as_deref())?;
            let key = pgp.unlocked_key(&own)?;
            let passcode = nail_pgp::autocrypt::new_passcode();
            let text = nail_pgp::autocrypt::SetupMessageText {
                body: crate::strings::message(locale.as_deref(), "autocrypt-setup-message-body"),
                html_intro: crate::strings::message(
                    locale.as_deref(),
                    "autocrypt-setup-message-html-intro",
                ),
            };
            let raw = nail_pgp::autocrypt::create_setup_message(
                &key,
                &identity.email,
                crypto.prefer_encrypt,
                &passcode,
                &text,
            )?;
            Ok((raw, passcode))
        })
        .await?;
    let blobs = service.blobs().clone();
    let size = raw.len() as u64;
    let hash = tokio::task::spawn_blocking(move || blobs.put(&raw)).await??;
    let mut draft = Draft::new(identity.id.clone());
    draft.to = vec![Address {
        name: identity.name.clone(),
        email: identity.email.clone(),
    }];
    nail_pgp::autocrypt::SETUP_SUBJECT.clone_into(&mut draft.subject);
    draft.attachments = vec![DraftAttachment {
        filename: "autocrypt-setup-message.eml".to_owned(),
        mime: "message/rfc822".to_owned(),
        size,
        blob: hash.as_str().to_owned(),
        content_id: None,
    }];
    draft.automatic = Some(AutomaticMail::AutocryptSetup);
    let draft_id = service.store_draft(draft, false).await?;
    let entry = service
        .send_draft(draft_id, Some(0), None, Some(SendCrypto::none()))
        .await?;
    Ok(Response::AutocryptSetupSent {
        outbox_id: entry.id,
        passcode,
    })
}

/// The stored security settings, or the defaults.
pub(crate) async fn security_settings(service: &Service) -> Result<SecuritySettings> {
    let store = service.store().clone();
    let stored = tokio::task::spawn_blocking(move || {
        store.setting::<SecuritySettings>(SECURITY_SETTINGS_KEY)
    })
    .await??;
    Ok(stored.unwrap_or_default())
}

async fn update_security_settings(
    service: &Service,
    mut settings: SecuritySettings,
) -> Result<SecuritySettings> {
    let config = service.inner.accounts.config();
    let stored = service.stored_accounts().await?;
    for account in &settings.index_decrypted {
        if config.account(account).is_none() && !stored.iter().any(|a| &a.id == account) {
            return Err(ApiError::invalid_request(format!(
                "index_decrypted names {account}, which is not a real account"
            ))
            .into());
        }
    }
    settings.index_decrypted.sort();
    settings.index_decrypted.dedup();
    let mut seen = Vec::new();
    settings.lookup_sources.retain(|s| {
        let new = !seen.contains(s);
        seen.push(*s);
        new
    });
    let store = service.store().clone();
    let saved = settings.clone();
    tokio::task::spawn_blocking(move || store.set_setting(SECURITY_SETTINGS_KEY, &saved)).await??;
    Ok(settings)
}

/// Decrypted indexing switched on or off for some accounts: their
/// encrypted messages are indexed again in the background (withdrawn
/// accounts lose their decrypted documents).
fn reindex_for_policy_change(
    service: &Service,
    previous: &SecuritySettings,
    settings: &SecuritySettings,
) {
    let old = IndexPolicy::from_settings(previous);
    let new = IndexPolicy::from_settings(settings);
    let mut accounts = new.withdrawn(&old);
    accounts.extend(new.granted(&old));
    if accounts.is_empty() || tokio::runtime::Handle::try_current().is_err() {
        return;
    }
    let service = service.clone();
    tokio::spawn(async move {
        let result = service
            .blocking(move |s| s.reindex_encrypted(&accounts, &new))
            .await;
        match result {
            Ok(count) => tracing::info!(count, "encrypted messages reindexed"),
            Err(err) => tracing::warn!(%err, "cannot reindex encrypted messages"),
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn openpgp_data_is_told_from_certificates() {
        assert!(is_openpgp(b"-----BEGIN PGP PUBLIC KEY BLOCK-----\n"));
        assert!(is_openpgp(&[0x99, 0x01]));
        assert!(is_openpgp(&[0xc6, 0x33]));
        assert!(!is_openpgp(b"-----BEGIN CERTIFICATE-----\n"));
        assert!(!is_openpgp(&[0x30, 0x82, 0x01]));
        assert!(!is_openpgp(b""));
    }

    #[test]
    fn keys_sort_own_first() {
        let key = |email: &str, own: bool| CryptoKey {
            id: KeyId::from(email),
            technology: CryptoTechnology::OpenPgp,
            fingerprint: email.into(),
            user_ids: Vec::new(),
            emails: vec![email.into()],
            algorithm: String::new(),
            created_at: String::new(),
            expires_at: None,
            expired: false,
            revoked: false,
            can_sign: true,
            can_encrypt: true,
            has_secret: own,
            secret_storage: None,
            source: KeySource::Imported,
            trust: nail_api::security::TrustLevel::Unknown,
            issuer: None,
            serial: None,
            chain: None,
            smartcard: None,
        };
        let sorted = sort_keys(vec![key("b@x", false), key("z@x", true), key("a@x", false)]);
        assert_eq!(
            sorted
                .iter()
                .map(|k| k.emails[0].as_str())
                .collect::<Vec<_>>(),
            ["z@x", "a@x", "b@x"]
        );
    }
}
