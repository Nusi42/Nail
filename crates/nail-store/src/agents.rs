//! Schema v14 (M4a, ADR 0079): confirmations of writes by MCP clients and
//! plug-ins, and their action policies ("always allow" / "ask").
//!
//! A confirmation is inserted pending ([`Store::insert_confirmation`]),
//! decided once ([`Store::decide_confirmation`], [`Store::expire_confirmations`],
//! [`Store::cancel_confirmation`]) and, when approved, finished
//! ([`Store::finish_confirmation`]). Its view (what the user sees) and
//! payload (what runs once approved) are sealed at rest; the payload is
//! taken out once ([`Store::take_confirmation_payload`]) and dropped with
//! the decision. Times are Unix seconds passed in by the caller.

use nail_api::agents::{
    ActionActor, ActionAttribution, ActionChange, ActionKind, ActionPolicy, ActionPolicyMode,
    ActionTarget, CONFIRMATION_HISTORY, Confirmation, ConfirmationFilter, ConfirmationSource,
    ConfirmationState, McpClientRef, NewConfirmation, check_policy,
};
use nail_api::{AccountId, ConfirmationId, LocalizedMessage};
use rusqlite::{OptionalExtension, Row, Transaction, params};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::error::{Error, Result};
use crate::store::Store;

const RFC3339: &str = "'%Y-%m-%dT%H:%M:%SZ'";

/// The sealed `view` column.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct View {
    summary: LocalizedMessage,
    targets: Vec<ActionTarget>,
    more_targets: u32,
    changes: Vec<ActionChange>,
    account_ids: Vec<AccountId>,
}

fn parse_id(id: &ConfirmationId) -> Result<i64> {
    id.as_str()
        .parse()
        .map_err(|_| Error::not_found("confirmation", id))
}

fn cols() -> String {
    format!(
        "id, actor_kind, actor_id, actor_name, via_client_id, via_client_name, kind, operation,
         nail_open('confirmations.view', view), state,
         strftime({RFC3339}, created_at, 'unixepoch'), strftime({RFC3339}, expires_at, 'unixepoch'),
         strftime({RFC3339}, decided_at, 'unixepoch'), decided_via, error"
    )
}

fn bad(idx: usize, what: &str) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(
        idx,
        rusqlite::types::Type::Text,
        format!("invalid {what}").into(),
    )
}

fn from_row(row: &Row<'_>) -> rusqlite::Result<Confirmation> {
    let actor_kind: String = row.get(1)?;
    let actor_id: String = row.get(2)?;
    let actor = ActionActor::from_parts(&actor_kind, &actor_id).ok_or_else(|| bad(1, "actor"))?;
    let via_id: Option<String> = row.get(4)?;
    let via_name: Option<String> = row.get(5)?;
    let kind: String = row.get(6)?;
    let kind = ActionKind::parse(&kind).ok_or_else(|| bad(6, "kind"))?;
    let view: String = row.get(8)?;
    let view: View = serde_json::from_str(&view).map_err(|_| bad(8, "view"))?;
    let state: String = row.get(9)?;
    let state = ConfirmationState::parse(&state).ok_or_else(|| bad(9, "state"))?;
    let decided_via: Option<String> = row.get(13)?;
    let error: Option<String> = row.get(14)?;
    Ok(Confirmation {
        id: ConfirmationId::new(row.get::<_, i64>(0)?.to_string()),
        requested_by: ActionAttribution {
            actor,
            name: row.get(3)?,
            on_behalf_of: via_id.map(|id| McpClientRef {
                client_id: id.into(),
                name: via_name.unwrap_or_default(),
            }),
        },
        kind,
        operation: row.get(7)?,
        summary: view.summary,
        targets: view.targets,
        more_targets: view.more_targets,
        changes: view.changes,
        account_ids: view.account_ids,
        created_at: row.get(10)?,
        expires_at: row.get(11)?,
        state,
        decided_at: row.get(12)?,
        decided_via: decided_via.as_deref().and_then(ConfirmationSource::parse),
        can_always_allow: kind.can_always_allow(),
        error: error.and_then(|e| serde_json::from_str(&e).ok()),
    })
}

fn get_in_tx(tx: &Transaction<'_>, n: i64) -> Result<Confirmation> {
    tx.query_row(
        &format!("SELECT {} FROM confirmations WHERE id = ?1", cols()),
        [n],
        from_row,
    )
    .optional()?
    .ok_or_else(|| Error::not_found("confirmation", n))
}

fn state_of(tx: &Transaction<'_>, n: i64) -> Result<ConfirmationState> {
    let state: String = tx
        .query_row("SELECT state FROM confirmations WHERE id = ?1", [n], |r| {
            r.get(0)
        })
        .optional()?
        .ok_or_else(|| Error::not_found("confirmation", n))?;
    ConfirmationState::parse(&state)
        .ok_or_else(|| Error::InvalidData(format!("confirmation state {state:?}")))
}

fn prune(tx: &Transaction<'_>) -> Result<()> {
    tx.execute(
        "DELETE FROM confirmations WHERE state <> 'pending' AND id NOT IN (
             SELECT id FROM confirmations WHERE state <> 'pending' ORDER BY id DESC LIMIT ?1)",
        [CONFIRMATION_HISTORY],
    )?;
    Ok(())
}

/// What [`Store::insert_confirmation`] did.
#[derive(Debug, Clone, PartialEq)]
pub enum InsertedConfirmation {
    /// A new pending confirmation.
    New(Confirmation),
    /// The actor used the idempotency key before: that confirmation (in
    /// whatever state it is now).
    Existing(Confirmation),
}

impl InsertedConfirmation {
    /// The confirmation either way.
    pub fn confirmation(&self) -> &Confirmation {
        match self {
            Self::New(c) | Self::Existing(c) => c,
        }
    }
}

impl Store {
    /// Stores a pending confirmation at `now` (Unix seconds), expiring
    /// `new.timeout_secs` later (the limits of
    /// [`NewConfirmation::normalize`] are applied). With an idempotency key
    /// the actor used before, the earlier confirmation is returned instead.
    /// Fails with [`Error::Conflict`] when the actor already has
    /// `max_pending` pending confirmations.
    pub fn insert_confirmation(
        &self,
        mut new: NewConfirmation,
        now: i64,
        max_pending: u32,
    ) -> Result<InsertedConfirmation> {
        let more_targets = new.normalize();
        let actor = new.requested_by.actor.clone();
        self.with_tx(|tx| {
            if let Some(key) = &new.idempotency_key {
                let existing: Option<i64> = tx
                    .query_row(
                        "SELECT id FROM confirmations
                         WHERE actor_kind = ?1 AND actor_id = ?2 AND idempotency_key = ?3",
                        params![actor.kind_str(), actor.id_str(), key],
                        |r| r.get(0),
                    )
                    .optional()?;
                if let Some(n) = existing {
                    return Ok(InsertedConfirmation::Existing(get_in_tx(tx, n)?));
                }
            }
            let pending: u32 = tx.query_row(
                "SELECT COUNT(*) FROM confirmations
                 WHERE actor_kind = ?1 AND actor_id = ?2 AND state = 'pending'",
                params![actor.kind_str(), actor.id_str()],
                |r| r.get(0),
            )?;
            if pending >= max_pending {
                return Err(Error::Conflict(format!(
                    "{} {} has {pending} pending confirmations",
                    actor.kind_str(),
                    actor.id_str()
                )));
            }
            let view = View {
                summary: new.summary.clone(),
                targets: new.targets.clone(),
                more_targets,
                changes: new.changes.clone(),
                account_ids: new.account_ids.clone(),
            };
            let via = new.requested_by.on_behalf_of.as_ref();
            let timeout = i64::try_from(new.timeout_secs).unwrap_or(i64::MAX / 2);
            tx.execute(
                "INSERT INTO confirmations (actor_kind, actor_id, actor_name, via_client_id,
                     via_client_name, kind, operation, view, payload, idempotency_key, created_at,
                     expires_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, nail_seal('confirmations.view', ?8),
                     nail_seal('confirmations.payload', ?9), ?10, ?11, ?12)",
                params![
                    actor.kind_str(),
                    actor.id_str(),
                    new.requested_by.name,
                    via.map(|v| v.client_id.as_str()),
                    via.map(|v| v.name.as_str()),
                    new.kind.as_str(),
                    new.operation,
                    serde_json::to_string(&view)?,
                    serde_json::to_string(&new.payload)?,
                    new.idempotency_key,
                    now,
                    now.saturating_add(timeout),
                ],
            )?;
            let n = tx.last_insert_rowid();
            Ok(InsertedConfirmation::New(get_in_tx(tx, n)?))
        })
    }

    /// One confirmation.
    pub fn get_confirmation(&self, id: &ConfirmationId) -> Result<Confirmation> {
        let n = parse_id(id)?;
        self.with_tx(|tx| get_in_tx(tx, n))
    }

    /// Confirmations: pending ones oldest first, otherwise newest first;
    /// optionally of one actor and below `before`.
    pub fn list_confirmations(
        &self,
        filter: ConfirmationFilter,
        actor: Option<&ActionActor>,
        before: Option<&ConfirmationId>,
        limit: u32,
    ) -> Result<Vec<Confirmation>> {
        let before = before.map(parse_id).transpose()?.unwrap_or(i64::MAX);
        let (state_sql, order) = match filter {
            ConfirmationFilter::Pending => ("state = 'pending'", "ASC"),
            ConfirmationFilter::Decided => ("state <> 'pending'", "DESC"),
            ConfirmationFilter::All => ("1", "DESC"),
        };
        let (kind, id) = actor.map_or((None, None), |a| (Some(a.kind_str()), Some(a.id_str())));
        self.with_tx(|tx| {
            let mut stmt = tx.prepare(&format!(
                "SELECT {} FROM confirmations
                 WHERE {state_sql} AND id < ?1
                   AND (?2 IS NULL OR (actor_kind = ?2 AND actor_id = ?3))
                 ORDER BY id {order} LIMIT ?4",
                cols()
            ))?;
            let rows = stmt
                .query_map(params![before, kind, id, limit], from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(rows)
        })
    }

    /// Takes the payload of a confirmation the caller is about to run
    /// (after approval): returned once, then cleared.
    pub fn take_confirmation_payload(&self, id: &ConfirmationId) -> Result<Option<Value>> {
        let n = parse_id(id)?;
        self.with_tx(|tx| {
            let payload: Option<String> = tx
                .query_row(
                    "SELECT nail_open('confirmations.payload', payload) FROM confirmations
                     WHERE id = ?1",
                    [n],
                    |r| r.get(0),
                )
                .optional()?
                .ok_or_else(|| Error::not_found("confirmation", n))?;
            tx.execute("UPDATE confirmations SET payload = NULL WHERE id = ?1", [n])?;
            payload
                .map(|p| serde_json::from_str(&p).map_err(Error::from))
                .transpose()
        })
    }

    /// Approves or denies a pending confirmation at `now`. An approval
    /// from a source that may not approve this kind
    /// ([`ConfirmationSource::may_approve`]) fails with
    /// [`Error::InvalidData`]; a confirmation that is no longer pending
    /// (or expired by `now`) fails with [`Error::Conflict`]. A denial drops
    /// the payload; an approval keeps it for
    /// [`Self::take_confirmation_payload`].
    pub fn decide_confirmation(
        &self,
        id: &ConfirmationId,
        approve: bool,
        source: ConfirmationSource,
        now: i64,
    ) -> Result<Confirmation> {
        let n = parse_id(id)?;
        self.with_tx(|tx| {
            let current = get_in_tx(tx, n)?;
            if current.state != ConfirmationState::Pending {
                return Err(Error::Conflict(format!(
                    "confirmation {n} is {}",
                    current.state.as_str()
                )));
            }
            let expires: i64 = tx.query_row(
                "SELECT expires_at FROM confirmations WHERE id = ?1",
                [n],
                |r| r.get(0),
            )?;
            if expires <= now {
                return Err(Error::Conflict(format!("confirmation {n} expired")));
            }
            if approve && !source.may_approve(current.kind) {
                return Err(Error::InvalidData(format!(
                    "{} cannot approve {}",
                    source.as_str(),
                    current.kind
                )));
            }
            let state = if approve {
                ConfirmationState::Approved
            } else {
                ConfirmationState::Denied
            };
            tx.execute(
                "UPDATE confirmations SET state = ?2, decided_at = ?3, decided_via = ?4,
                     payload = CASE WHEN ?5 THEN payload ELSE NULL END
                 WHERE id = ?1",
                params![n, state.as_str(), now, source.as_str(), approve],
            )?;
            prune(tx)?;
            get_in_tx(tx, n)
        })
    }

    /// Records how an approved confirmation's action ended: `Ok` keeps it
    /// approved, `Err` marks it failed with the user-facing text. Clears
    /// the payload.
    pub fn finish_confirmation(
        &self,
        id: &ConfirmationId,
        outcome: std::result::Result<(), LocalizedMessage>,
    ) -> Result<Confirmation> {
        let n = parse_id(id)?;
        self.with_tx(|tx| {
            if state_of(tx, n)? != ConfirmationState::Approved {
                return Err(Error::Conflict(format!("confirmation {n} is not approved")));
            }
            match outcome {
                Ok(()) => {
                    tx.execute("UPDATE confirmations SET payload = NULL WHERE id = ?1", [n])?
                }
                Err(error) => tx.execute(
                    "UPDATE confirmations SET state = 'failed', payload = NULL, error = ?2
                     WHERE id = ?1",
                    params![n, serde_json::to_string(&error)?],
                )?,
            };
            get_in_tx(tx, n)
        })
    }

    /// Cancels a pending confirmation at `now` (its caller went away, the
    /// item is gone); `None` when it was no longer pending.
    pub fn cancel_confirmation(
        &self,
        id: &ConfirmationId,
        now: i64,
    ) -> Result<Option<Confirmation>> {
        let n = parse_id(id)?;
        self.with_tx(|tx| {
            let changed = tx.execute(
                "UPDATE confirmations SET state = 'cancelled', decided_at = ?2,
                     decided_via = 'system', payload = NULL
                 WHERE id = ?1 AND state = 'pending'",
                params![n, now],
            )?;
            if changed == 0 {
                return Ok(None);
            }
            prune(tx)?;
            get_in_tx(tx, n).map(Some)
        })
    }

    /// Cancels every pending confirmation of an actor (a revoked client, a
    /// stopped plug-in); returns them.
    pub fn cancel_confirmations_of(
        &self,
        actor: &ActionActor,
        now: i64,
    ) -> Result<Vec<Confirmation>> {
        self.with_tx(|tx| {
            let ids: Vec<i64> = {
                let mut stmt = tx.prepare(
                    "SELECT id FROM confirmations
                     WHERE actor_kind = ?1 AND actor_id = ?2 AND state = 'pending'",
                )?;
                stmt.query_map(params![actor.kind_str(), actor.id_str()], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?
            };
            let mut out = Vec::new();
            for n in ids {
                tx.execute(
                    "UPDATE confirmations SET state = 'cancelled', decided_at = ?2,
                         decided_via = 'system', payload = NULL
                     WHERE id = ?1",
                    params![n, now],
                )?;
                out.push(get_in_tx(tx, n)?);
            }
            prune(tx)?;
            Ok(out)
        })
    }

    /// Expires every pending confirmation whose time ran out by `now`;
    /// returns them.
    pub fn expire_confirmations(&self, now: i64) -> Result<Vec<Confirmation>> {
        self.with_tx(|tx| {
            let ids: Vec<i64> = {
                let mut stmt = tx.prepare(
                    "SELECT id FROM confirmations WHERE state = 'pending' AND expires_at <= ?1",
                )?;
                stmt.query_map([now], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?
            };
            let mut out = Vec::new();
            for n in ids {
                tx.execute(
                    "UPDATE confirmations SET state = 'expired', decided_at = ?2,
                         decided_via = 'system', payload = NULL
                     WHERE id = ?1",
                    params![n, now],
                )?;
                out.push(get_in_tx(tx, n)?);
            }
            prune(tx)?;
            Ok(out)
        })
    }

    /// The earliest expiry of a pending confirmation (Unix seconds), for
    /// the core's timer.
    pub fn next_confirmation_expiry(&self) -> Result<Option<i64>> {
        self.with_conn(|conn| {
            Ok(conn.query_row(
                "SELECT MIN(expires_at) FROM confirmations WHERE state = 'pending'",
                [],
                |r| r.get(0),
            )?)
        })
    }

    /// An actor's policy for every kind ([`ActionPolicy::table`]).
    pub fn action_policies(&self, actor: &ActionActor) -> Result<Vec<ActionPolicy>> {
        self.with_tx(|tx| Ok(ActionPolicy::table(&stored_policies(tx, actor)?)))
    }

    /// An actor's policy for one kind.
    pub fn action_policy(&self, actor: &ActionActor, kind: ActionKind) -> Result<ActionPolicy> {
        self.with_tx(|tx| {
            let stored = stored_policies(tx, actor)?;
            let mode = stored.iter().find(|(k, _)| *k == kind).map(|(_, m)| *m);
            Ok(ActionPolicy::effective(kind, mode))
        })
    }

    /// Sets (or with `None` resets) an actor's policy for one kind at
    /// `now`; returns the actor's whole table. `allow` for a destructive
    /// kind fails with [`Error::InvalidData`] (check
    /// [`check_policy`] first for the user-facing issue).
    pub fn set_action_policy(
        &self,
        actor: &ActionActor,
        kind: ActionKind,
        mode: Option<ActionPolicyMode>,
        now: i64,
    ) -> Result<Vec<ActionPolicy>> {
        if let Some(mode) = mode
            && check_policy(kind, mode).is_err()
        {
            return Err(Error::InvalidData(format!(
                "{kind} cannot be allowed without asking"
            )));
        }
        self.with_tx(|tx| {
            match mode {
                Some(mode) => tx.execute(
                    "INSERT INTO action_policies (actor_kind, actor_id, kind, mode, updated_at)
                     VALUES (?1, ?2, ?3, ?4, ?5)
                     ON CONFLICT (actor_kind, actor_id, kind)
                     DO UPDATE SET mode = excluded.mode, updated_at = excluded.updated_at",
                    params![
                        actor.kind_str(),
                        actor.id_str(),
                        kind.as_str(),
                        mode.as_str(),
                        now
                    ],
                )?,
                None => tx.execute(
                    "DELETE FROM action_policies
                     WHERE actor_kind = ?1 AND actor_id = ?2 AND kind = ?3",
                    params![actor.kind_str(), actor.id_str(), kind.as_str()],
                )?,
            };
            Ok(ActionPolicy::table(&stored_policies(tx, actor)?))
        })
    }

    /// Removes an actor's policies (client revoked, plug-in uninstalled).
    pub fn remove_action_policies(&self, actor: &ActionActor) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "DELETE FROM action_policies WHERE actor_kind = ?1 AND actor_id = ?2",
                params![actor.kind_str(), actor.id_str()],
            )?;
            Ok(())
        })
    }
}

fn stored_policies(
    tx: &Transaction<'_>,
    actor: &ActionActor,
) -> Result<Vec<(ActionKind, ActionPolicyMode)>> {
    let mut stmt = tx.prepare_cached(
        "SELECT kind, mode FROM action_policies WHERE actor_kind = ?1 AND actor_id = ?2",
    )?;
    let rows = stmt
        .query_map(params![actor.kind_str(), actor.id_str()], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    Ok(rows
        .into_iter()
        .filter_map(|(k, m)| Some((ActionKind::parse(&k)?, ActionPolicyMode::parse(&m)?)))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::agents::{ActionTargetKind, MAX_CONFIRMATION_TARGETS};
    use serde_json::json;

    fn store() -> Store {
        Store::open_in_memory().unwrap()
    }

    fn new(actor: ActionActor, kind: ActionKind, key: Option<&str>) -> NewConfirmation {
        NewConfirmation {
            requested_by: ActionAttribution {
                actor,
                name: "Claude".into(),
                on_behalf_of: None,
            },
            kind,
            operation: "delete_messages".into(),
            summary: LocalizedMessage::new("confirmation-summary-trash").arg("count", 2),
            targets: (0..60)
                .map(|i| ActionTarget {
                    kind: ActionTargetKind::Message,
                    id: format!("m{i}"),
                    label: format!("Subject {i}"),
                })
                .collect(),
            changes: vec![ActionChange {
                field: LocalizedMessage::new("action-field-folder"),
                before: Some("Inbox".into()),
                after: Some("Trash".into()),
            }],
            account_ids: vec!["work".into()],
            payload: json!({"message_ids": ["m0", "m1"]}),
            idempotency_key: key.map(str::to_owned),
            timeout_secs: 300,
        }
    }

    const NOW: i64 = 1_790_000_000;

    #[test]
    fn insert_list_decide() {
        let store = store();
        let client = ActionActor::mcp_client("mcp-1");
        let inserted = store
            .insert_confirmation(new(client.clone(), ActionKind::Trash, None), NOW, 20)
            .unwrap();
        let InsertedConfirmation::New(c) = inserted else {
            panic!("expected a new confirmation");
        };
        assert_eq!(c.state, ConfirmationState::Pending);
        assert_eq!(c.targets.len(), MAX_CONFIRMATION_TARGETS);
        assert_eq!(c.more_targets, 10);
        assert!(!c.can_always_allow);
        assert_eq!(c.created_at, "2026-09-21T14:13:20Z");
        assert_eq!(c.expires_at, "2026-09-21T14:18:20Z");
        assert_eq!(store.get_confirmation(&c.id).unwrap(), c);

        let pending = store
            .list_confirmations(ConfirmationFilter::Pending, None, None, 10)
            .unwrap();
        assert_eq!(pending, std::slice::from_ref(&c));
        let mine = store
            .list_confirmations(ConfirmationFilter::All, Some(&client), None, 10)
            .unwrap();
        assert_eq!(mine.len(), 1);
        let others = store
            .list_confirmations(
                ConfirmationFilter::All,
                Some(&ActionActor::plugin("x")),
                None,
                10,
            )
            .unwrap();
        assert!(others.is_empty());

        // Elicitation may not approve a destructive kind.
        assert!(matches!(
            store.decide_confirmation(&c.id, true, ConfirmationSource::Elicitation, NOW + 1),
            Err(Error::InvalidData(_))
        ));
        let approved = store
            .decide_confirmation(&c.id, true, ConfirmationSource::Nail, NOW + 1)
            .unwrap();
        assert_eq!(approved.state, ConfirmationState::Approved);
        assert_eq!(approved.decided_via, Some(ConfirmationSource::Nail));
        assert!(matches!(
            store.decide_confirmation(&c.id, false, ConfirmationSource::Nail, NOW + 2),
            Err(Error::Conflict(_))
        ));
        assert_eq!(
            store.take_confirmation_payload(&c.id).unwrap(),
            Some(json!({"message_ids": ["m0", "m1"]}))
        );
        assert_eq!(store.take_confirmation_payload(&c.id).unwrap(), None);
        let failed = store
            .finish_confirmation(&c.id, Err(LocalizedMessage::new("error-network")))
            .unwrap();
        assert_eq!(failed.state, ConfirmationState::Failed);
        assert_eq!(failed.error.unwrap().id, "error-network");
        assert!(
            store
                .list_confirmations(ConfirmationFilter::Pending, None, None, 10)
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            store
                .list_confirmations(ConfirmationFilter::Decided, None, None, 10)
                .unwrap()
                .len(),
            1
        );
        assert!(
            store
                .get_confirmation(&ConfirmationId::new("nope"))
                .unwrap_err()
                .is_not_found()
        );
    }

    #[test]
    fn denial_expiry_cancel_and_limits() {
        let store = store();
        let plugin = ActionActor::plugin("follow-up");
        let a = store
            .insert_confirmation(new(plugin.clone(), ActionKind::Send, None), NOW, 3)
            .unwrap();
        let b = store
            .insert_confirmation(new(plugin.clone(), ActionKind::Send, None), NOW + 10, 3)
            .unwrap();
        let c = store
            .insert_confirmation(new(plugin.clone(), ActionKind::Send, None), NOW + 20, 3)
            .unwrap();
        assert!(c.confirmation().can_always_allow);
        assert!(matches!(
            store.insert_confirmation(new(plugin.clone(), ActionKind::Send, None), NOW, 3),
            Err(Error::Conflict(_))
        ));
        // Elicitation may approve a non-destructive kind; deny drops the payload.
        let denied = store
            .decide_confirmation(
                &a.confirmation().id,
                false,
                ConfirmationSource::Elicitation,
                NOW + 1,
            )
            .unwrap();
        assert_eq!(denied.state, ConfirmationState::Denied);
        assert_eq!(
            store
                .take_confirmation_payload(&a.confirmation().id)
                .unwrap(),
            None
        );
        assert_eq!(store.next_confirmation_expiry().unwrap(), Some(NOW + 310));
        let expired = store.expire_confirmations(NOW + 310).unwrap();
        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0].id, b.confirmation().id);
        assert_eq!(expired[0].state, ConfirmationState::Expired);
        // Deciding after the expiry time is refused even before the sweep.
        assert!(matches!(
            store.decide_confirmation(
                &c.confirmation().id,
                true,
                ConfirmationSource::Nail,
                NOW + 9999
            ),
            Err(Error::Conflict(_))
        ));
        let cancelled = store.cancel_confirmations_of(&plugin, NOW + 30).unwrap();
        assert_eq!(cancelled.len(), 1);
        assert_eq!(cancelled[0].state, ConfirmationState::Cancelled);
        assert_eq!(
            store
                .cancel_confirmation(&c.confirmation().id, NOW + 31)
                .unwrap(),
            None
        );
        assert_eq!(store.next_confirmation_expiry().unwrap(), None);
    }

    #[test]
    fn idempotency_keys_return_the_first_confirmation() {
        let store = store();
        let client = ActionActor::mcp_client("mcp-1");
        let first = store
            .insert_confirmation(new(client.clone(), ActionKind::Send, Some("k1")), NOW, 20)
            .unwrap();
        let again = store
            .insert_confirmation(new(client.clone(), ActionKind::Send, Some("k1")), NOW, 20)
            .unwrap();
        assert!(matches!(again, InsertedConfirmation::Existing(_)));
        assert_eq!(again.confirmation().id, first.confirmation().id);
        // Another actor's key is independent.
        let other = store
            .insert_confirmation(
                new(
                    ActionActor::mcp_client("mcp-2"),
                    ActionKind::Send,
                    Some("k1"),
                ),
                NOW,
                20,
            )
            .unwrap();
        assert!(matches!(other, InsertedConfirmation::New(_)));
    }

    #[test]
    fn plugins_acting_for_clients_are_attributed() {
        let store = store();
        let mut request = new(ActionActor::plugin("crm"), ActionKind::Task, None);
        request.requested_by.on_behalf_of = Some(McpClientRef {
            client_id: "mcp-3".into(),
            name: "Agent".into(),
        });
        let c = store.insert_confirmation(request, NOW, 20).unwrap();
        let by = &c.confirmation().requested_by;
        assert_eq!(by.on_behalf_of.as_ref().unwrap().name, "Agent");
        assert_eq!(by.actor, ActionActor::plugin("crm"));
    }

    #[test]
    fn decided_history_is_pruned() {
        let store = store();
        let actor = ActionActor::mcp_client("mcp-1");
        let mut last = None;
        for i in 0..(CONFIRMATION_HISTORY + 5) {
            let c = store
                .insert_confirmation(
                    new(actor.clone(), ActionKind::Draft, None),
                    NOW + i64::from(i),
                    u32::MAX,
                )
                .unwrap();
            store
                .decide_confirmation(
                    &c.confirmation().id,
                    false,
                    ConfirmationSource::Nail,
                    NOW + i64::from(i),
                )
                .unwrap();
            last = Some(c.confirmation().id.clone());
        }
        let history = store
            .list_confirmations(ConfirmationFilter::Decided, None, None, u32::MAX)
            .unwrap();
        assert_eq!(history.len(), CONFIRMATION_HISTORY as usize);
        assert_eq!(history[0].id, last.unwrap());
        // Paging with `before`.
        let page = store
            .list_confirmations(ConfirmationFilter::Decided, None, Some(&history[9].id), 5)
            .unwrap();
        assert_eq!(page[0].id, history[10].id);
    }

    #[test]
    fn policies() {
        let store = store();
        let client = ActionActor::mcp_client("mcp-1");
        let table = store.action_policies(&client).unwrap();
        assert_eq!(table.len(), ActionKind::ALL.len());
        assert!(table.iter().all(|p| !p.customized));
        assert!(
            store
                .action_policy(&client, ActionKind::Send)
                .unwrap()
                .asks()
        );

        let table = store
            .set_action_policy(
                &client,
                ActionKind::Send,
                Some(ActionPolicyMode::Allow),
                NOW,
            )
            .unwrap();
        let send = table.iter().find(|p| p.kind == ActionKind::Send).unwrap();
        assert!(send.customized && !send.asks());
        assert!(
            !store
                .action_policy(&client, ActionKind::Send)
                .unwrap()
                .asks()
        );
        // Another actor is unaffected.
        assert!(
            store
                .action_policy(&ActionActor::plugin("x"), ActionKind::Send)
                .unwrap()
                .asks()
        );
        assert!(matches!(
            store.set_action_policy(
                &client,
                ActionKind::PermanentDelete,
                Some(ActionPolicyMode::Allow),
                NOW
            ),
            Err(Error::InvalidData(_))
        ));
        store
            .set_action_policy(&client, ActionKind::Send, None, NOW)
            .unwrap();
        assert!(
            store
                .action_policy(&client, ActionKind::Send)
                .unwrap()
                .asks()
        );
        store
            .set_action_policy(&client, ActionKind::Draft, Some(ActionPolicyMode::Ask), NOW)
            .unwrap();
        store.remove_action_policies(&client).unwrap();
        assert!(
            store
                .action_policies(&client)
                .unwrap()
                .iter()
                .all(|p| !p.customized)
        );
    }

    #[test]
    fn the_schema_refuses_allowing_destructive_kinds() {
        let store = store();
        let err = store.with_conn(|conn| {
            conn.execute(
                "INSERT INTO action_policies VALUES ('plugin', 'x', 'trash', 'allow', 0)",
                [],
            )?;
            Ok(())
        });
        assert!(err.is_err());
    }
}
