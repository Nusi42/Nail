//! Every Fluent id the contract names (error codes, validation issues,
//! notification texts of the core) exists in the English catalogs
//! (`locales/en/*.ftl`), and the German catalogs are the same files
//! (ADR 0043: localized core messages).

use std::collections::BTreeSet;
use std::path::PathBuf;

use nail_api::ErrorCode;
use nail_api::agents::{
    ACTION_FIELD_IDS, ActionKind, ActionPolicyIssue, CONFIRMATION_SUMMARY_IDS, ConfirmationState,
};
use nail_api::desktop::{
    DefaultAppState, DefaultAppTarget, InstallKind, InstallScope, UpdateChannel,
    UpdateDisabledReason, UpdateFailure,
};
use nail_api::exchange::oof::AutomaticRepliesIssue;
use nail_api::exchange::{
    CategoryColor, ExchangeCategoryIssue, ExchangeConfigIssue, ExchangeEngine,
    ExchangeMailboxIssue, ExchangeMailboxState, ExchangeWarning, InferenceClassification,
};
use nail_api::gmail::{GmailCategory, GmailLabelIssue};
use nail_api::mapi::MapiResult;
use nail_api::mcp::prompts::McpPrompt;
use nail_api::mcp::{McpRateLimitIssue, McpSettingsIssue};
use nail_api::pim::PimIssue;
use nail_api::pim::source::{PimSourceIssue, PimSourceState};
use nail_api::plugins::{
    PluginHookKind, PluginJobOutcome, PluginManifestIssueKind, PluginPermission, PluginStopReason,
    PluginUiIssueKind,
};
use nail_api::scheduling::{FreeBusyIssue, ItipIssue};
use nail_api::sieve::{ManageSieveIssue, VacationIssue};
use nail_api::triage::{JunkIssue, NotificationIssue};

fn locales() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../locales")
}

fn files(locale: &str) -> BTreeSet<String> {
    std::fs::read_dir(locales().join(locale))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".ftl"))
        .collect()
}

/// Message ids defined in a locale (lines `id = …` at the start).
fn ids(locale: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    for file in files(locale) {
        let text = std::fs::read_to_string(locales().join(locale).join(&file)).unwrap();
        for line in text.lines() {
            if let Some((id, _)) = line.split_once(" =")
                && nail_api::localized::is_message_id(id)
            {
                out.insert(id.to_owned());
            }
        }
    }
    out
}

#[test]
fn contract_message_ids_exist_in_english() {
    let en = ids("en");
    let mut wanted: Vec<String> = ErrorCode::ALL.iter().map(|c| c.message_id()).collect();
    wanted.extend(VacationIssue::ALL.iter().map(|i| i.message_id().to_owned()));
    wanted.extend(
        ManageSieveIssue::ALL
            .iter()
            .map(|i| i.message_id().to_owned()),
    );
    wanted.extend(JunkIssue::ALL.iter().map(|i| i.message_id().to_owned()));
    wanted.extend(
        NotificationIssue::ALL
            .iter()
            .map(|i| i.message_id().to_owned()),
    );
    wanted.extend(
        McpSettingsIssue::ALL
            .iter()
            .map(|i| i.message_id().to_owned()),
    );
    // M2d (ADR 0050): plug-in manifests, UI trees, stops, permissions.
    wanted.extend(
        PluginManifestIssueKind::ALL
            .iter()
            .map(|i| i.message_id().to_owned()),
    );
    wanted.extend(
        PluginUiIssueKind::ALL
            .iter()
            .map(|i| i.message_id().to_owned()),
    );
    wanted.extend(
        PluginStopReason::ALL
            .iter()
            .map(|r| r.message_id().to_owned()),
    );
    wanted.extend(PluginPermission::label_ids().into_iter().map(str::to_owned));
    wanted.push("plugin-settings-invalid".to_owned());
    // M3a (ADR 0058): PIM validation issues, source issues and states, the
    // reminder notification.
    wanted.extend(PimIssue::ALL.iter().map(|i| i.message_id().to_owned()));
    wanted.extend(
        PimSourceIssue::ALL
            .iter()
            .map(|i| i.message_id().to_owned()),
    );
    wanted.extend(
        PimSourceState::ALL
            .iter()
            .map(|s| s.message_id().to_owned()),
    );
    wanted.extend(
        [
            "notification-reminder-title",
            "notification-reminder-body",
            "notification-reminder-body-location",
        ]
        .map(str::to_owned),
    );
    // M3b (ADR 0065): invitation and free/busy problems, Gmail label
    // problems and category names.
    wanted.extend(ItipIssue::ALL.iter().map(|i| i.message_id().to_owned()));
    wanted.extend(FreeBusyIssue::ALL.iter().map(|i| i.message_id().to_owned()));
    wanted.extend(
        GmailLabelIssue::ALL
            .iter()
            .map(|i| i.message_id().to_owned()),
    );
    wanted.extend(GmailCategory::ALL.iter().map(|c| c.message_id()));
    // M3c (ADR 0072): Exchange configuration problems, warnings, mailbox /
    // category / automatic-reply problems, Focused Inbox, engine and
    // colour names.
    wanted.extend(
        ExchangeConfigIssue::ALL
            .iter()
            .map(|i| i.message_id().to_owned()),
    );
    wanted.extend(
        ExchangeWarning::ALL
            .iter()
            .map(|w| w.message_id().to_owned()),
    );
    wanted.extend(
        ExchangeMailboxIssue::ALL
            .iter()
            .map(|i| i.message_id().to_owned()),
    );
    wanted.extend(ExchangeMailboxState::ALL.iter().map(|s| s.message_id()));
    wanted.extend(
        ExchangeCategoryIssue::ALL
            .iter()
            .map(|i| i.message_id().to_owned()),
    );
    wanted.extend(CategoryColor::ALL.iter().map(|c| c.message_id()));
    wanted.extend(
        AutomaticRepliesIssue::ALL
            .iter()
            .map(|i| i.message_id().to_owned()),
    );
    wanted.extend(
        InferenceClassification::ALL
            .iter()
            .map(|c| c.message_id().to_owned()),
    );
    wanted.extend(
        ExchangeEngine::ALL
            .iter()
            .map(|e| e.message_id().to_owned()),
    );
    wanted.extend(
        [
            "config-exchange-needs-host",
            "config-exchange-no-smtp",
            "config-auth-windows-needs-exchange",
            "config-sspi-package-invalid",
        ]
        .map(str::to_owned),
    );
    // M4a (ADR 0079): action kinds, confirmation states / summaries / fields,
    // policy and rate-limit problems, prompts, hooks, job outcomes.
    wanted.extend(ActionKind::ALL.iter().map(|k| k.message_id()));
    wanted.extend(ConfirmationState::ALL.iter().map(|s| s.message_id()));
    wanted.extend(
        ActionPolicyIssue::ALL
            .iter()
            .map(|i| i.message_id().to_owned()),
    );
    wanted.extend(CONFIRMATION_SUMMARY_IDS.iter().map(|id| (*id).to_owned()));
    wanted.extend(ACTION_FIELD_IDS.iter().map(|id| (*id).to_owned()));
    wanted.extend(
        McpRateLimitIssue::ALL
            .iter()
            .map(|i| i.message_id().to_owned()),
    );
    wanted.extend(
        McpPrompt::ALL
            .iter()
            .flat_map(|p| [p.description_id(), p.instructions_id()]),
    );
    wanted.extend(
        PluginHookKind::ALL
            .iter()
            .map(|h| h.message_id().to_owned()),
    );
    wanted.extend(
        [
            PluginJobOutcome::Ok,
            PluginJobOutcome::Error,
            PluginJobOutcome::Timeout,
            PluginJobOutcome::CpuQuota,
            PluginJobOutcome::Overlap,
        ]
        .iter()
        .map(|o| o.message_id()),
    );
    // M4b (ADR 0087): installation facts, update channels / failures /
    // disabled reasons, default apps, MAPI results, core-built desktop texts.
    wanted.extend(InstallKind::ALL.iter().map(|k| k.message_id()));
    wanted.extend(InstallScope::ALL.iter().map(|s| s.message_id()));
    wanted.extend(UpdateChannel::ALL.iter().map(|c| c.message_id()));
    wanted.extend(
        UpdateDisabledReason::ALL
            .iter()
            .map(|r| r.message_id().to_owned()),
    );
    wanted.extend(UpdateFailure::ALL.iter().map(|f| f.message_id().to_owned()));
    wanted.extend(
        DefaultAppTarget::ALL
            .iter()
            .map(|t| t.message_id().to_owned()),
    );
    wanted.extend(
        DefaultAppState::ALL
            .iter()
            .map(|s| s.message_id().to_owned()),
    );
    wanted.extend(MapiResult::ALL.iter().map(|r| r.message_id()));
    wanted.extend(
        [
            "update-channel-locked",
            "desktop-unavailable",
            "mapi-draft-from-program",
        ]
        .map(str::to_owned),
    );
    // Built by the core (`TestNotification`, the notification policy).
    wanted.extend(
        [
            "notification-test-title",
            "notification-test-body",
            "notification-new-mail-title",
            "notification-new-mail-body",
            "notification-snooze-woke-title",
        ]
        .map(str::to_owned),
    );
    let missing: Vec<&String> = wanted.iter().filter(|id| !en.contains(*id)).collect();
    assert!(missing.is_empty(), "missing in locales/en: {missing:?}");
    for id in &wanted {
        assert!(nail_api::localized::is_message_id(id), "{id}");
    }
}

#[test]
fn german_catalogs_are_the_same_files() {
    assert_eq!(files("de"), files("en"));
    for file in [
        "core.ftl",
        "core-sieve.ftl",
        "core-triage.ftl",
        "core-plugins.ftl",
        "plugins.ftl",
        "core-pim.ftl",
        "core-ical.ftl",
        "core-dav.ftl",
        "pim.ftl",
        "core-scheduling.ftl",
        "core-gmail.ftl",
        "scheduling.ftl",
        "gmail.ftl",
        "core-exchange.ftl",
        "core-graph.ftl",
        "core-ews.ftl",
        "exchange.ftl",
        "core-agents.ftl",
        "core-mcp.ftl",
        "agents.ftl",
        "core-desktop.ftl",
        "system.ftl",
    ] {
        assert!(files("en").contains(file), "{file}");
    }
}
