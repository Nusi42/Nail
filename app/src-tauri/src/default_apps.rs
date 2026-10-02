//! Default-app registration (M4b WP113, ADR 0087): which schemes and file
//! types Nail is registered for and whether Windows uses it as the default.
//!
//! * Registered: `RegisteredApplications\Nail` points to Nail's
//!   Capabilities key, whose `URLAssociations` / `FileAssociations` name
//!   Nail's ProgId per scheme / extension (the NSIS hooks write them per
//!   user, the MSI per machine).
//! * Default: the user's choice — `UserChoiceLatest\ProgId` (Windows 11
//!   24H2 and later) or `UserChoice` below
//!   `Shell\Associations\UrlAssociations\<scheme>` /
//!   `Explorer\FileExts\<.ext>` — names Nail's ProgId. No choice = another
//!   app (Windows' own default); an unreadable key = unknown.
//! * Simple MAPI: `Software\Clients\Mail\Nail` with a DLL path; the
//!   default client is `Software\Clients\Mail`'s default value, per user
//!   before per machine.
//!
//! Windows never lets an app make itself the default, so Nail only opens
//! Settings → Default apps ([`nail_api::desktop::default_apps_settings_uri`]).

use nail_api::desktop::{
    DefaultAppAssociation, DefaultAppState, DefaultAppTarget, DefaultAppsStatus, MapiClientStatus,
    REGISTERED_APP_NAME, registry as keys,
};
use nail_api::mapi::registry as mapi_keys;

use crate::registry::{Hive, Registry};

const URL_CHOICES: &str = r"Software\Microsoft\Windows\Shell\Associations\UrlAssociations";
const FILE_CHOICES: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts";

/// The Capabilities key `RegisteredApplications\Nail` names, in the hive
/// that has it (per user first).
fn capabilities(registry: &dyn Registry) -> Option<(Hive, String)> {
    [Hive::CurrentUser, Hive::LocalMachine]
        .into_iter()
        .find_map(|hive| {
            registry
                .string(hive, keys::REGISTERED_APPLICATIONS_KEY, REGISTERED_APP_NAME)
                .map(|path| (hive, path.trim().trim_start_matches('\\').to_owned()))
        })
}

/// The ProgId Windows uses for a scheme / extension by the user's choice:
/// `Ok(None)` = no choice made.
fn user_choice(
    registry: &dyn Registry,
    target: DefaultAppTarget,
) -> Result<Option<String>, String> {
    let base = if target.is_scheme() {
        format!(r"{URL_CHOICES}\{}", target.windows_name())
    } else {
        format!(r"{FILE_CHOICES}\{}", target.windows_name())
    };
    for (key, name) in [
        (format!(r"{base}\UserChoiceLatest\ProgId"), "ProgId"),
        (format!(r"{base}\UserChoiceLatest"), "ProgId"),
        (format!(r"{base}\UserChoice"), "ProgId"),
    ] {
        if let Some(value) = registry.get(Hive::CurrentUser, &key, name)? {
            let prog_id = value.as_string();
            if !prog_id.trim().is_empty() {
                return Ok(Some(prog_id.trim().to_owned()));
            }
        }
    }
    Ok(None)
}

/// Reads the registration.
pub fn status(registry: &dyn Registry) -> DefaultAppsStatus {
    let caps = capabilities(registry);
    let associations = DefaultAppTarget::ALL
        .into_iter()
        .map(|target| {
            let sub = if target.is_scheme() {
                "URLAssociations"
            } else {
                "FileAssociations"
            };
            let prog_id = caps.as_ref().and_then(|(hive, path)| {
                registry.string(*hive, &format!(r"{path}\{sub}"), target.windows_name())
            });
            let state = match (&prog_id, user_choice(registry, target)) {
                (_, Err(err)) => {
                    tracing::debug!(%err, ?target, "cannot read the user's choice");
                    DefaultAppState::Unknown
                }
                (Some(ours), Ok(Some(choice))) if ours.eq_ignore_ascii_case(&choice) => {
                    DefaultAppState::Default
                }
                (None, Ok(Some(choice))) if choice.to_ascii_lowercase().starts_with("nail.") => {
                    DefaultAppState::Default
                }
                (_, Ok(_)) => DefaultAppState::Other,
            };
            DefaultAppAssociation {
                target,
                registered: prog_id.is_some(),
                state,
            }
        })
        .collect();
    DefaultAppsStatus {
        registered: caps.is_some(),
        associations,
        mapi: mapi_status(registry),
    }
}

/// Nail as the Simple MAPI client.
fn mapi_status(registry: &dyn Registry) -> MapiClientStatus {
    let registered = [mapi_keys::DLL_PATH_EX_VALUE, mapi_keys::DLL_PATH_VALUE]
        .into_iter()
        .any(|name| {
            registry
                .string(Hive::LocalMachine, mapi_keys::NAIL_MAIL_CLIENT_KEY, name)
                .is_some()
        });
    let chosen = [Hive::CurrentUser, Hive::LocalMachine]
        .into_iter()
        .map(|hive| registry.get(hive, mapi_keys::MAIL_CLIENTS_KEY, ""))
        .find(|value| !matches!(value, Ok(None)));
    let state = match chosen {
        Some(Ok(Some(name)))
            if name
                .as_string()
                .trim()
                .eq_ignore_ascii_case(mapi_keys::CLIENT_NAME) =>
        {
            DefaultAppState::Default
        }
        Some(Err(_)) => DefaultAppState::Unknown,
        _ => DefaultAppState::Other,
    };
    MapiClientStatus { registered, state }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::{MemoryRegistry, RegValue};
    use pretty_assertions::assert_eq;

    /// What the NSIS hooks write per user.
    fn registered(hive: Hive) -> MemoryRegistry {
        let reg = MemoryRegistry::new();
        reg.put(
            hive,
            keys::REGISTERED_APPLICATIONS_KEY,
            "Nail",
            RegValue::Str(keys::CAPABILITIES_KEY.into()),
        );
        let caps = keys::CAPABILITIES_KEY;
        for (scheme, prog) in [
            ("mailto", "Nail.Url.mailto"),
            ("webcal", "Nail.Url.webcal"),
            ("webcals", "Nail.Url.webcal"),
        ] {
            reg.put(
                hive,
                &format!(r"{caps}\URLAssociations"),
                scheme,
                RegValue::Str(prog.into()),
            );
        }
        for (ext, prog) in [
            (".eml", "Nail.eml"),
            (".ics", "Nail.ics"),
            (".vcf", "Nail.vcf"),
        ] {
            reg.put(
                hive,
                &format!(r"{caps}\FileAssociations"),
                ext,
                RegValue::Str(prog.into()),
            );
        }
        reg
    }

    #[test]
    fn nothing_registered_in_a_development_build() {
        let status = status(&MemoryRegistry::new());
        assert!(!status.registered);
        assert_eq!(status.associations.len(), DefaultAppTarget::ALL.len());
        assert!(
            status
                .associations
                .iter()
                .all(|a| !a.registered && a.state == DefaultAppState::Other)
        );
        assert!(!status.mapi.registered);
        assert_eq!(status.mapi.state, DefaultAppState::Other);
    }

    #[test]
    fn the_users_choice_decides_the_default() {
        let reg = registered(Hive::CurrentUser);
        reg.put(
            Hive::CurrentUser,
            &format!(r"{URL_CHOICES}\mailto\UserChoice"),
            "ProgId",
            RegValue::Str("Nail.Url.mailto".into()),
        );
        reg.put(
            Hive::CurrentUser,
            &format!(r"{FILE_CHOICES}\.eml\UserChoice"),
            "ProgId",
            RegValue::Str("Outlook.File.eml.15".into()),
        );
        // Windows 11 24H2 keeps the newer choice in UserChoiceLatest.
        reg.put(
            Hive::CurrentUser,
            &format!(r"{FILE_CHOICES}\.ics\UserChoice"),
            "ProgId",
            RegValue::Str("Outlook.ics".into()),
        );
        reg.put(
            Hive::CurrentUser,
            &format!(r"{FILE_CHOICES}\.ics\UserChoiceLatest\ProgId"),
            "ProgId",
            RegValue::Str("nail.ics".into()),
        );
        let status = status(&reg);
        assert!(status.registered);
        let state = |t: DefaultAppTarget| {
            status
                .associations
                .iter()
                .find(|a| a.target == t)
                .map(|a| (a.registered, a.state))
        };
        assert_eq!(
            state(DefaultAppTarget::Mailto),
            Some((true, DefaultAppState::Default))
        );
        assert_eq!(
            state(DefaultAppTarget::Eml),
            Some((true, DefaultAppState::Other))
        );
        assert_eq!(
            state(DefaultAppTarget::Ics),
            Some((true, DefaultAppState::Default))
        );
        assert_eq!(
            state(DefaultAppTarget::Webcal),
            Some((true, DefaultAppState::Other))
        );
        assert!(status.is_default_mail_app());
    }

    #[test]
    fn a_per_machine_registration_counts() {
        let reg = registered(Hive::LocalMachine);
        let status = status(&reg);
        assert!(status.registered);
        assert!(status.associations.iter().all(|a| a.registered));
    }

    #[test]
    fn the_mapi_client_and_its_default() {
        let reg = MemoryRegistry::new();
        reg.put(
            Hive::LocalMachine,
            mapi_keys::NAIL_MAIL_CLIENT_KEY,
            mapi_keys::DLL_PATH_EX_VALUE,
            RegValue::Str(r"C:\Program Files\Nail\mapi\x64\NailMapi.dll".into()),
        );
        reg.put(
            Hive::LocalMachine,
            mapi_keys::MAIL_CLIENTS_KEY,
            "",
            RegValue::Str("Nail".into()),
        );
        let status = status(&reg);
        assert!(status.mapi.registered);
        assert_eq!(status.mapi.state, DefaultAppState::Default);
        // The user's own choice comes first.
        reg.put(
            Hive::CurrentUser,
            mapi_keys::MAIL_CLIENTS_KEY,
            "",
            RegValue::Str("Microsoft Outlook".into()),
        );
        assert_eq!(super::status(&reg).mapi.state, DefaultAppState::Other);
    }
}
