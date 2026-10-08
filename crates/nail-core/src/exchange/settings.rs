//! Categories, automatic replies and inbox rules of an Exchange account
//! (ADR 0077).
//!
//! * The server's master category list and automatic replies replace the
//!   stored ones (queued local changes win until they are replayed).
//! * **Rules**: the server's inbox rules are read; with `server-rules` on,
//!   every Nail rule that applies to the account is planned with
//!   `nail_exchange::mapping::rules::plan`. A rule that maps is written as
//!   an inbox rule (created, or replaced when the digest of what was
//!   placed changed); placed rules whose Nail rule changed its placement,
//!   was removed or switched off — or all of them when `server-rules` is
//!   off — are deleted. At the engine seam `move_to_folder` /
//!   `copy_to_folder` carry Exchange folder ids; the store and the UI see
//!   local folder ids. Arrival rules skip the placed rules
//!   ([`Service::server_rules`](crate::service::Service)).
//! * The generic vacation reply (`GetVacation` / `UpdateVacation`) of an
//!   Exchange account maps to automatic replies
//!   (`nail_exchange::mapping::oof`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use nail_api::exchange::engine::ExchangeEngine;
use nail_api::exchange::rules::{ExchangeRuleStatus, ExchangeRulesState, ExchangeRulesStatus};
use nail_api::exchange::{ExchangeArea, ExchangeInboxRule};
use nail_api::rules::FolderRef;
use nail_api::sieve::{RulePlacement, VacationSettings};
use nail_api::{AccountConfig, AccountId, ApiError, FolderId, FolderRole, RuleId};
use nail_exchange::mapping::rules::{PlacementContext, RulePlan, applies_to, digest, plan};
use nail_store::exchange::{ExchangeCategoryRecord, ExchangeOp};

use super::{RunningEngine, changed, exchange_parts};
use crate::error::{CoreError, Result};
use crate::service::Service;
use nail_util::time::unix_now;

/// Reads (and writes) the account's settings: categories, automatic
/// replies, rules.
pub(crate) async fn sync(
    service: &Service,
    account: &AccountConfig,
    running: &Arc<RunningEngine>,
) -> Result<()> {
    let engine = running.engine.as_ref();
    let categories = engine.categories().await.map_err(CoreError::from)?;
    let records: Vec<ExchangeCategoryRecord> = categories
        .into_iter()
        .map(|c| ExchangeCategoryRecord {
            id: c.id.into(),
            name: c.name,
            color: c.color,
            sync: nail_api::exchange::ExchangeSync::Synced,
        })
        .collect();
    let id = account.id.clone();
    let categories_changed = service
        .store()
        .call(move |s| {
            let before = s.exchange_category_records(&id)?;
            s.replace_exchange_categories(&id, &records)?;
            Ok(before != s.exchange_category_records(&id)?)
        })
        .await?;
    let replies = engine.automatic_replies().await.map_err(CoreError::from)?;
    let id = account.id.clone();
    let now = unix_now();
    let replies_changed = service
        .store()
        .call(move |s| {
            let before = s.exchange_automatic_replies(&id)?;
            s.set_exchange_automatic_replies(&id, &replies, now)?;
            Ok(
                before.settings != s.exchange_automatic_replies(&id)?.settings
                    || before.fetched_at.is_none(),
            )
        })
        .await?;
    let mut areas = Vec::new();
    if categories_changed {
        areas.push(ExchangeArea::Categories);
    }
    if replies_changed {
        areas.push(ExchangeArea::AutomaticReplies);
    }
    if !areas.is_empty() {
        changed(service, &account.id, &areas);
    }
    sync_rules(service, account, engine).await
}

/// Local folder ↔ Exchange folder ids of the own mailbox.
async fn folder_maps(
    service: &Service,
    account: &AccountId,
) -> Result<(HashMap<FolderId, String>, HashMap<String, FolderId>)> {
    let id = account.clone();
    let folders = service
        .store()
        .call(move |s| s.exchange_folders(&id))
        .await?;
    let mut to_remote = HashMap::new();
    let mut to_local = HashMap::new();
    for f in folders.into_iter().filter(|f| f.mailbox.is_none()) {
        to_remote.insert(f.folder_id.clone(), f.remote_id.clone());
        to_local.insert(f.remote_id, f.folder_id);
    }
    Ok((to_remote, to_local))
}

fn map_folders(rule: &mut ExchangeInboxRule, map: &dyn Fn(&FolderId) -> Option<FolderId>) {
    for slot in [
        &mut rule.actions.move_to_folder,
        &mut rule.actions.copy_to_folder,
    ] {
        if let Some(id) = slot.as_ref() {
            *slot = map(id);
        }
    }
}

/// The plans of every Nail rule that applies to the account, in
/// `rules.kdl` order.
async fn plans(service: &Service, account: &AccountConfig) -> Result<Vec<(RuleId, RulePlan)>> {
    let Some((_, options)) = exchange_parts(account) else {
        return Ok(Vec::new());
    };
    let id = account.id.clone();
    let folders = service.store().call(move |s| s.list_folders(&id)).await?;
    let mut roles: HashMap<FolderRole, FolderId> = HashMap::new();
    let mut paths: HashMap<String, FolderId> = HashMap::new();
    for f in &folders {
        if f.path.starts_with('@') {
            continue;
        }
        if f.role != FolderRole::Other {
            roles.entry(f.role).or_insert_with(|| f.id.clone());
        }
        paths.insert(f.path.to_lowercase(), f.id.clone());
    }
    let resolve = move |folder: &FolderRef| -> Option<FolderId> {
        match folder {
            FolderRef::Role { role, .. } => roles.get(role).cloned(),
            FolderRef::Path { path, .. } => paths.get(&path.to_lowercase()).cloned(),
        }
    };
    let ctx = PlacementContext {
        account_id: &account.id,
        server_rules: options.server_rules(),
        resolve_folder: &resolve,
    };
    Ok(service
        .inner
        .rules
        .rules()
        .into_iter()
        .filter(|r| applies_to(r, &account.id))
        .map(|r| {
            let plan = plan(&r, &ctx);
            (r.id.clone(), plan)
        })
        .collect())
}

/// Reads the server's rules and writes the placed ones.
async fn sync_rules(
    service: &Service,
    account: &AccountConfig,
    engine: &dyn ExchangeEngine,
) -> Result<()> {
    let id = account.id.clone();
    let kind = engine.kind();
    service
        .store()
        .call(move |s| {
            s.set_exchange_rules_state(&id, kind, ExchangeRulesState::Syncing, None, None)
        })
        .await?;
    changed(service, &account.id, &[ExchangeArea::Rules]);
    let result = place_rules(service, account, engine).await;
    let (state, error) = match &result {
        Ok(()) => {
            let on = exchange_parts(account).is_some_and(|(_, o)| o.server_rules());
            (
                if on {
                    ExchangeRulesState::InSync
                } else {
                    ExchangeRulesState::Disabled
                },
                None,
            )
        }
        Err(err) => (
            ExchangeRulesState::Failed,
            Some(ApiError::new(err.code(), err.to_string())),
        ),
    };
    let id = account.id.clone();
    let now = unix_now();
    service
        .store()
        .call(move |s| s.set_exchange_rules_state(&id, kind, state, Some(now), error.as_ref()))
        .await?;
    changed(service, &account.id, &[ExchangeArea::Rules]);
    result
}

async fn read_rules(
    service: &Service,
    account: &AccountId,
    engine: &dyn ExchangeEngine,
    to_local: &HashMap<String, FolderId>,
) -> Result<()> {
    let mut rules = engine.inbox_rules().await.map_err(CoreError::from)?;
    for rule in &mut rules {
        map_folders(rule, &|remote| {
            to_local
                .get(remote.as_str())
                .cloned()
                .or(Some(remote.clone()))
        });
    }
    let id = account.clone();
    service
        .store()
        .call(move |s| s.replace_exchange_rules(&id, &rules))
        .await?;
    Ok(())
}

async fn place_rules(
    service: &Service,
    account: &AccountConfig,
    engine: &dyn ExchangeEngine,
) -> Result<()> {
    let (to_remote, to_local) = folder_maps(service, &account.id).await?;
    read_rules(service, &account.id, engine, &to_local).await?;
    let plans = plans(service, account).await?;
    let id = account.id.clone();
    let stored = service.store().call(move |s| s.exchange_rules(&id)).await?;
    let on_server: HashSet<String> = stored.iter().map(|r| r.id.clone()).collect();
    let mut wrote = false;
    let mut wanted: HashSet<RuleId> = HashSet::new();
    for (sequence, (rule_id, plan)) in plans.iter().enumerate() {
        let Some(inbox_rule) = plan.inbox_rule.clone() else {
            continue;
        };
        if plan.placement != RulePlacement::Server {
            continue;
        }
        wanted.insert(rule_id.clone());
        let hash = digest(&inbox_rule);
        let (id, rid) = (account.id.clone(), rule_id.clone());
        let placed = service
            .store()
            .call(move |s| s.exchange_rule_placement(&id, &rid))
            .await?;
        let current = placed
            .as_ref()
            .filter(|(server, _)| on_server.contains(server));
        if current.is_some_and(|(_, d)| d.as_deref() == Some(hash.as_str())) {
            continue;
        }
        let mut remote_rule = inbox_rule.clone();
        remote_rule.sequence = u32::try_from(sequence + 1).unwrap_or(u32::MAX);
        map_folders(&mut remote_rule, &|local| {
            to_remote.get(local).map(|r| FolderId::new(r.clone()))
        });
        let server_id = match current {
            Some((server, _)) => {
                remote_rule.id.clone_from(server);
                engine
                    .update_inbox_rule(&remote_rule)
                    .await
                    .map_err(CoreError::from)?;
                server.clone()
            }
            None => engine
                .create_inbox_rule(&remote_rule)
                .await
                .map_err(CoreError::from)?,
        };
        wrote = true;
        // The new rule must be in the store before its placement is.
        read_rules(service, &account.id, engine, &to_local).await?;
        let (id, rid) = (account.id.clone(), rule_id.clone());
        service
            .store()
            .call(move |s| s.set_exchange_rule_placement(&id, &server_id, Some((&rid, &hash))))
            .await?;
    }
    // Placed rules that should not be there any more.
    for rule in &stored {
        let Some(nail) = &rule.nail_rule_id else {
            continue;
        };
        if wanted.contains(nail) {
            continue;
        }
        engine
            .delete_inbox_rule(&rule.id)
            .await
            .map_err(CoreError::from)?;
        wrote = true;
    }
    if wrote {
        read_rules(service, &account.id, engine, &to_local).await?;
    }
    Ok(())
}

/// `GetExchangeRules` / `SyncExchangeRules`.
pub(crate) async fn rules_status(
    service: &Service,
    account: &AccountConfig,
) -> Result<ExchangeRulesStatus> {
    let id = account.id.clone();
    let (server_rules, (state, synced_at, error)) = service
        .store()
        .call(move |s| Ok((s.exchange_rules(&id)?, s.exchange_rules_state(&id)?)))
        .await?;
    let on = exchange_parts(account).is_some_and(|(_, o)| o.server_rules());
    let plans = plans(service, account).await?;
    let rules = plans
        .into_iter()
        .map(|(rule_id, plan)| {
            let placed = server_rules
                .iter()
                .find(|r| r.nail_rule_id.as_ref() == Some(&rule_id))
                .map(|r| r.id.clone());
            let mut reasons = plan.reasons;
            if plan.placement == RulePlacement::Server && placed.is_none() {
                reasons.push(nail_api::LocalizedMessage::new(
                    nail_exchange::texts::RULE_PENDING,
                ));
            }
            ExchangeRuleStatus {
                rule_id,
                placement: plan.placement,
                server_rule_id: placed,
                reasons,
            }
        })
        .collect();
    let localized = error.as_ref().and_then(|e| e.localized.as_deref().cloned());
    Ok(ExchangeRulesStatus {
        account_id: account.id.clone(),
        state: if on {
            state
        } else {
            ExchangeRulesState::Disabled
        },
        rules,
        server_rules,
        synced_at: nail_util::time::rfc3339_utc_opt(synced_at),
        error: error.map(|e| e.code),
        localized,
    })
}

/// Marks the rules pending and wakes the account's engine.
pub(crate) async fn request_rules_sync(service: &Service, account: &AccountConfig) -> Result<()> {
    let Some((_, options)) = exchange_parts(account) else {
        return Ok(());
    };
    let id = account.id.clone();
    let kind = options.engine;
    service
        .store()
        .call(move |s| {
            s.set_exchange_rules_state(&id, kind, ExchangeRulesState::Pending, None, None)
        })
        .await?;
    if let Some(running) = service.inner.exchange.running(&account.id) {
        running.settings_wake.notify_one();
    }
    changed(service, &account.id, &[ExchangeArea::Rules]);
    Ok(())
}

/// Rules changed (`rules.kdl`, account settings): every Exchange account
/// places them again.
pub(crate) fn queue_rules(service: &Service, only: Option<&AccountId>) {
    let config = service.inner.accounts.config();
    for account in &config.accounts {
        if exchange_parts(account).is_none() || only.is_some_and(|o| o != &account.id) {
            continue;
        }
        let (service, account) = (service.clone(), account.clone());
        tokio::spawn(async move {
            if let Err(err) = request_rules_sync(&service, &account).await {
                tracing::debug!(account = %account.id, %err, "cannot queue the Exchange rules");
            }
        });
    }
}

/// The stored server rule `id` (`not_found` when unknown).
pub(crate) async fn server_rule(
    service: &Service,
    account: &AccountId,
    rule_id: &str,
) -> Result<ExchangeInboxRule> {
    let id = account.clone();
    let rules = service.store().call(move |s| s.exchange_rules(&id)).await?;
    let mut rule = rules
        .into_iter()
        .find(|r| r.id == rule_id)
        .ok_or_else(|| ApiError::not_found(format!("inbox rule {rule_id}")))?;
    // Back to Exchange folder ids for the engine.
    let (to_remote, _) = folder_maps(service, account).await?;
    map_folders(&mut rule, &|local| {
        Some(
            to_remote
                .get(local)
                .map_or_else(|| local.clone(), |r| FolderId::new(r.clone())),
        )
    });
    Ok(rule)
}

/// Nail rules the server runs for an Exchange account (arrival rules skip
/// them).
pub(crate) async fn placed_rules(service: &Service, account: &AccountId) -> Vec<RuleId> {
    let id = account.clone();
    match service.store().call(move |s| s.exchange_rules(&id)).await {
        Ok(rules) => rules
            .into_iter()
            .filter(|r| r.enabled && !r.in_error)
            .filter_map(|r| r.nail_rule_id)
            .collect(),
        Err(err) => {
            tracing::warn!(%account, %err, "cannot read the Exchange rules");
            Vec::new()
        }
    }
}

/// The zone the generic vacation reply's dates are in.
fn vacation_zone() -> chrono_tz::Tz {
    crate::pim::local_zone()
}

/// `GetVacation` of an Exchange account: the automatic replies as a
/// vacation reply.
pub(crate) async fn vacation(service: &Service, account: &AccountId) -> Result<VacationSettings> {
    let replies = super::replies(service, account).await?;
    Ok(nail_exchange::mapping::oof::vacation_of(&replies.settings, vacation_zone()).value)
}

/// `UpdateVacation` of an Exchange account: queued as automatic replies.
pub(crate) async fn set_vacation(
    service: &Service,
    account: &AccountId,
    settings: &VacationSettings,
) -> Result<()> {
    let current = super::replies(service, account).await?;
    let mapped = nail_exchange::mapping::oof::automatic_replies_of(
        settings,
        vacation_zone(),
        chrono::Utc::now(),
        Some(&current.settings),
    )
    .value;
    let issues = mapped.issues();
    if let Some(first) = issues.first() {
        return Err(ApiError::invalid_request(format!(
            "invalid automatic replies ({})",
            first.message_id()
        ))
        .with_localized(nail_api::LocalizedMessage::new(first.message_id()))
        .into());
    }
    let id = account.clone();
    service
        .store()
        .call(move |s| {
            s.enqueue_exchange_op(&id, &ExchangeOp::SetAutomaticReplies { settings: mapped })
        })
        .await?;
    service.inner.engines.request_replay(account);
    changed(service, account, &[ExchangeArea::AutomaticReplies]);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::exchange::rules::{ExchangeRuleActions, ExchangeRuleConditions};

    fn rule(move_to: Option<&str>) -> ExchangeInboxRule {
        ExchangeInboxRule {
            id: "r1".into(),
            name: "Lists".into(),
            sequence: 1,
            enabled: true,
            conditions: ExchangeRuleConditions::default(),
            exceptions: ExchangeRuleConditions::default(),
            actions: ExchangeRuleActions {
                move_to_folder: move_to.map(FolderId::new),
                copy_to_folder: Some(FolderId::new("local-copy")),
                ..ExchangeRuleActions::default()
            },
            has_unsupported: false,
            in_error: false,
            nail_rule_id: None,
        }
    }

    #[test]
    fn folder_ids_are_translated_both_ways() {
        let mut r = rule(Some("local-lists"));
        let to_remote: HashMap<FolderId, String> = [
            (FolderId::new("local-lists"), "AAMkLists".to_owned()),
            (FolderId::new("local-copy"), "AAMkCopy".to_owned()),
        ]
        .into();
        map_folders(&mut r, &|l| {
            to_remote.get(l).map(|x| FolderId::new(x.clone()))
        });
        assert_eq!(r.actions.move_to_folder, Some(FolderId::new("AAMkLists")));
        assert_eq!(r.actions.copy_to_folder, Some(FolderId::new("AAMkCopy")));
        // Digests ignore ids and sequences.
        let mut other = r.clone();
        other.id = "other".into();
        other.sequence = 9;
        assert_eq!(digest(&r), digest(&other));
        let mut none = rule(None);
        map_folders(&mut none, &|_| None);
        assert_eq!(none.actions.move_to_folder, None);
        assert_eq!(none.actions.copy_to_folder, None);
    }
}
