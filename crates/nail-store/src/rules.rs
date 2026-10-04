//! Rule runtime state (schema v4, ADR 0022). The rules themselves live in
//! `rules.kdl`; the store keeps:
//!
//! * per-rule state for schedules ([`RuleState`]),
//! * the run history: every run ([`RuleRunRecord`], ids `run-<n>`) with
//!   each action it took, the operation it queued and the operation that
//!   undoes it ([`RuleRunAction`]), so a run can be undone as one unit,
//! * the "rule inbox": new messages waiting for the arrival rules, so a
//!   restart does not skip them.

use nail_api::rules::{RuleAction, RuleRunCause, RuleRunState};
use nail_api::{MessageId, RuleId, RuleRunId};
use rusqlite::{OptionalExtension, Row, params};

use crate::error::{Error, Result};
use crate::queue::{OpId, Operation};
use crate::store::Store;

fn run_id(n: i64) -> RuleRunId {
    RuleRunId(format!("run-{n}"))
}

fn parse_run_id(id: &RuleRunId) -> Result<i64> {
    id.as_str()
        .strip_prefix("run-")
        .and_then(|n| n.parse().ok())
        .ok_or_else(|| Error::not_found("rule run", id))
}

fn cause_str(cause: RuleRunCause) -> &'static str {
    match cause {
        RuleRunCause::Arrival => "arrival",
        RuleRunCause::OnSend => "on-send",
        RuleRunCause::Schedule => "schedule",
        RuleRunCause::Manual => "manual",
    }
}

fn state_str(state: RuleRunState) -> &'static str {
    match state {
        RuleRunState::Running => "running",
        RuleRunState::Done => "done",
        RuleRunState::Failed => "failed",
        RuleRunState::Undone => "undone",
    }
}

fn from_text<T: serde::de::DeserializeOwned>(idx: usize, s: &str) -> rusqlite::Result<T> {
    serde_json::from_value(serde_json::Value::String(s.to_owned())).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

fn from_json<T: serde::de::DeserializeOwned>(idx: usize, s: &str) -> rusqlite::Result<T> {
    serde_json::from_str(s).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(idx, rusqlite::types::Type::Text, Box::new(e))
    })
}

/// Runtime state of one rule.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RuleState {
    /// Last run (Unix seconds).
    pub last_run_at: Option<i64>,
    /// Next scheduled run (Unix seconds).
    pub next_run_at: Option<i64>,
    /// Runs so far.
    pub run_count: u32,
}

/// One run as stored. The core turns it into a [`nail_api::RuleRun`]
/// (formatting times and op ids).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleRunRecord {
    /// The run.
    pub id: RuleRunId,
    /// The rule.
    pub rule_id: RuleId,
    /// The rule's name at the time of the run.
    pub rule_name: String,
    /// What started it.
    pub cause: RuleRunCause,
    /// Current state.
    pub state: RuleRunState,
    /// Start (Unix seconds).
    pub started_at: i64,
    /// End (Unix seconds).
    pub finished_at: Option<i64>,
    /// Messages matched.
    pub matched: u32,
    /// Diagnostic detail of a failure.
    pub message: Option<String>,
    /// Operations the run queued, in order.
    pub op_ids: Vec<OpId>,
    /// `done`, and every action that queued an operation has an inverse.
    pub undoable: bool,
}

/// One action of a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RuleRunAction {
    /// The message it acted on.
    pub message_id: MessageId,
    /// The operation it queued, if any.
    pub op_id: Option<OpId>,
    /// The action.
    pub action: RuleAction,
    /// The operation that undoes it; `None` = cannot be undone (a sent
    /// forward, a permanent delete).
    pub undo: Option<Operation>,
}

/// What [`Store::undo_rule_run`] did.
#[derive(Debug)]
pub struct RuleRunUndo {
    /// The run, now `undone`.
    pub record: RuleRunRecord,
    /// The inverse operations it queued (newest action first).
    pub queued: Vec<crate::queue::Enqueued>,
    /// Inverses skipped because they no longer apply.
    pub skipped: usize,
}

const RUN_COLS: &str =
    "id, rule_id, rule_name, cause, state, started_at, finished_at, matched, message";

fn run_from_row(row: &Row<'_>) -> rusqlite::Result<RuleRunRecord> {
    let cause: String = row.get(3)?;
    let state: String = row.get(4)?;
    Ok(RuleRunRecord {
        id: run_id(row.get(0)?),
        rule_id: RuleId(row.get(1)?),
        rule_name: row.get(2)?,
        cause: from_text(3, &cause)?,
        state: from_text(4, &state)?,
        started_at: row.get(5)?,
        finished_at: row.get(6)?,
        matched: row.get(7)?,
        message: row.get(8)?,
        op_ids: Vec::new(),
        undoable: false,
    })
}

fn complete_run(conn: &rusqlite::Connection, mut run: RuleRunRecord) -> Result<RuleRunRecord> {
    let n = parse_run_id(&run.id)?;
    let mut stmt = conn.prepare_cached(
        "SELECT op_id, undo IS NOT NULL FROM rule_run_actions WHERE run_id = ?1 ORDER BY seq",
    )?;
    let rows = stmt
        .query_map([n], |r| {
            Ok((r.get::<_, Option<i64>>(0)?, r.get::<_, bool>(1)?))
        })?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    run.op_ids = rows.iter().filter_map(|(op, _)| op.map(OpId)).collect();
    run.undoable = run.state == RuleRunState::Done
        && !run.op_ids.is_empty()
        && rows.iter().all(|(op, undo)| op.is_none() || *undo);
    Ok(run)
}

impl Store {
    /// A rule's runtime state (default when it never ran).
    pub fn rule_state(&self, rule: &RuleId) -> Result<RuleState> {
        self.with_conn(|conn| {
            Ok(conn
                .query_row(
                    "SELECT last_run_at, next_run_at, run_count FROM rule_state WHERE rule_id = ?1",
                    [rule.as_str()],
                    |r| {
                        Ok(RuleState {
                            last_run_at: r.get(0)?,
                            next_run_at: r.get(1)?,
                            run_count: r.get(2)?,
                        })
                    },
                )
                .optional()?
                .unwrap_or_default())
        })
    }

    /// Replaces a rule's runtime state.
    pub fn set_rule_state(&self, rule: &RuleId, state: &RuleState) -> Result<()> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO rule_state (rule_id, last_run_at, next_run_at, run_count)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (rule_id) DO UPDATE SET last_run_at = excluded.last_run_at,
                     next_run_at = excluded.next_run_at, run_count = excluded.run_count",
                params![
                    rule.as_str(),
                    state.last_run_at,
                    state.next_run_at,
                    state.run_count
                ],
            )?;
            Ok(())
        })
    }

    /// Forgets the state of rules that no longer exist in `rules.kdl`.
    /// Their run history stays. Returns how many were removed.
    pub fn retain_rule_states(&self, existing: &[RuleId]) -> Result<usize> {
        self.with_tx(|tx| {
            let ids: Vec<String> = {
                let mut stmt = tx.prepare("SELECT rule_id FROM rule_state")?;
                stmt.query_map([], |r| r.get(0))?
                    .collect::<rusqlite::Result<_>>()?
            };
            let mut removed = 0;
            for id in ids {
                if !existing.iter().any(|r| r.as_str() == id) {
                    removed += tx.execute("DELETE FROM rule_state WHERE rule_id = ?1", [id])?;
                }
            }
            Ok(removed)
        })
    }

    /// Starts a run (state `running`) at `at` (Unix seconds).
    pub fn start_rule_run(
        &self,
        rule: &RuleId,
        rule_name: &str,
        cause: RuleRunCause,
        at: i64,
    ) -> Result<RuleRunId> {
        self.with_conn(|conn| {
            conn.execute(
                "INSERT INTO rule_runs (rule_id, rule_name, cause, started_at) VALUES (?1, ?2, ?3, ?4)",
                params![rule.as_str(), rule_name, cause_str(cause), at],
            )?;
            Ok(run_id(conn.last_insert_rowid()))
        })
    }

    /// Appends an action to a running run.
    pub fn record_rule_action(&self, run: &RuleRunId, action: &RuleRunAction) -> Result<()> {
        let n = parse_run_id(run)?;
        let action_json = serde_json::to_string(&action.action)?;
        let undo_json = action
            .undo
            .as_ref()
            .map(serde_json::to_string)
            .transpose()?;
        self.with_tx(|tx| {
            let state: String = tx
                .query_row("SELECT state FROM rule_runs WHERE id = ?1", [n], |r| {
                    r.get(0)
                })
                .optional()?
                .ok_or_else(|| Error::not_found("rule run", run))?;
            if state != "running" {
                return Err(Error::Conflict(format!("rule run {run} is {state}")));
            }
            tx.execute(
                "INSERT INTO rule_run_actions (run_id, seq, message_id, op_id, action, undo)
                 VALUES (?1, (SELECT COALESCE(MAX(seq), 0) + 1 FROM rule_run_actions WHERE run_id = ?1),
                         ?2, ?3, ?4, ?5)",
                params![
                    n,
                    action.message_id.as_str(),
                    action.op_id.map(|o| o.0),
                    action_json,
                    undo_json
                ],
            )?;
            Ok(())
        })
    }

    /// Finishes a running run as `done` or `failed`.
    pub fn finish_rule_run(
        &self,
        run: &RuleRunId,
        state: RuleRunState,
        matched: u32,
        message: Option<&str>,
        at: i64,
    ) -> Result<RuleRunRecord> {
        if !matches!(state, RuleRunState::Done | RuleRunState::Failed) {
            return Err(Error::InvalidData(format!(
                "a run cannot finish as {}",
                state_str(state)
            )));
        }
        let n = parse_run_id(run)?;
        self.with_conn(|conn| {
            let changed = conn.execute(
                "UPDATE rule_runs SET state = ?2, matched = ?3, message = ?4, finished_at = ?5
                 WHERE id = ?1 AND state = 'running'",
                params![n, state_str(state), matched, message, at],
            )?;
            if changed == 0 {
                return Err(if Self::run_exists(conn, n)? {
                    Error::Conflict(format!("rule run {run} is not running"))
                } else {
                    Error::not_found("rule run", run)
                });
            }
            Ok(())
        })?;
        self.get_rule_run(run)
    }

    fn run_exists(conn: &rusqlite::Connection, n: i64) -> Result<bool> {
        Ok(conn
            .query_row("SELECT 1 FROM rule_runs WHERE id = ?1", [n], |_| Ok(()))
            .optional()?
            .is_some())
    }

    /// Marks a `done` run as undone (after the core queued the inverse
    /// operations). Fails with [`Error::Conflict`] unless it is undoable.
    pub fn mark_rule_run_undone(&self, run: &RuleRunId) -> Result<RuleRunRecord> {
        let record = self.get_rule_run(run)?;
        if !record.undoable {
            return Err(Error::Conflict(format!("rule run {run} cannot be undone")));
        }
        let n = parse_run_id(run)?;
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE rule_runs SET state = 'undone' WHERE id = ?1 AND state = 'done'",
                [n],
            )?;
            Ok(())
        })?;
        self.get_rule_run(run)
    }

    /// Undoes a run in one transaction: queues the inverse operations of
    /// its actions, newest first, and marks it `undone`. An inverse that no
    /// longer applies (its message was deleted or expunged, or the user
    /// already moved it back by hand) is skipped instead of failing the
    /// whole run, so the other messages are still restored. Fails with
    /// [`Error::Conflict`] unless the run can be undone; then nothing is
    /// queued and the run keeps its state.
    pub fn undo_rule_run(&self, run: &RuleRunId) -> Result<RuleRunUndo> {
        let n = parse_run_id(run)?;
        let undone = self.with_tx(|tx| {
            let record = tx
                .query_row(
                    &format!("SELECT {RUN_COLS} FROM rule_runs WHERE id = ?1"),
                    [n],
                    run_from_row,
                )
                .optional()?
                .ok_or_else(|| Error::not_found("rule run", run))?;
            if !complete_run(tx, record)?.undoable {
                return Err(Error::Conflict(format!("rule run {run} cannot be undone")));
            }
            let inverses: Vec<Operation> = {
                let mut stmt = tx.prepare_cached(
                    "SELECT undo FROM rule_run_actions
                     WHERE run_id = ?1 AND op_id IS NOT NULL AND undo IS NOT NULL
                     ORDER BY seq DESC",
                )?;
                stmt.query_map([n], |r| {
                    let undo: String = r.get(0)?;
                    from_json(0, &undo)
                })?
                .collect::<rusqlite::Result<_>>()?
            };
            let mut queued = Vec::with_capacity(inverses.len());
            let mut skipped = 0;
            for op in &inverses {
                tx.execute_batch("SAVEPOINT undo_op")?;
                match crate::queue::enqueue_in_tx(tx, op) {
                    Ok(enqueued) => {
                        tx.execute_batch("RELEASE undo_op")?;
                        queued.push(enqueued);
                    }
                    Err(
                        err @ (Error::NotFound { .. } | Error::Conflict(_) | Error::InvalidData(_)),
                    ) => {
                        tx.execute_batch("ROLLBACK TO undo_op; RELEASE undo_op")?;
                        tracing::debug!(%err, run = %run, "an inverse no longer applies; skipped");
                        skipped += 1;
                    }
                    Err(err) => return Err(err),
                }
            }
            let changed = tx.execute(
                "UPDATE rule_runs SET state = 'undone' WHERE id = ?1 AND state = 'done'",
                [n],
            )?;
            if changed == 0 {
                return Err(Error::Conflict(format!("rule run {run} cannot be undone")));
            }
            Ok((queued, skipped))
        })?;
        Ok(RuleRunUndo {
            record: self.get_rule_run(run)?,
            queued: undone.0,
            skipped: undone.1,
        })
    }

    /// One run.
    pub fn get_rule_run(&self, run: &RuleRunId) -> Result<RuleRunRecord> {
        let n = parse_run_id(run)?;
        self.with_conn(|conn| {
            let record = conn
                .query_row(
                    &format!("SELECT {RUN_COLS} FROM rule_runs WHERE id = ?1"),
                    [n],
                    run_from_row,
                )
                .optional()?
                .ok_or_else(|| Error::not_found("rule run", run))?;
            complete_run(conn, record)
        })
    }

    /// Recent runs, newest first, of one rule or all.
    pub fn list_rule_runs(&self, rule: Option<&RuleId>, limit: u32) -> Result<Vec<RuleRunRecord>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(&format!(
                "SELECT {RUN_COLS} FROM rule_runs WHERE ?1 IS NULL OR rule_id = ?1
                 ORDER BY id DESC LIMIT ?2"
            ))?;
            let records = stmt
                .query_map(params![rule.map(RuleId::as_str), limit], run_from_row)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            records.into_iter().map(|r| complete_run(conn, r)).collect()
        })
    }

    /// The actions of a run, in order.
    pub fn rule_run_actions(&self, run: &RuleRunId) -> Result<Vec<RuleRunAction>> {
        let n = parse_run_id(run)?;
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT message_id, op_id, action, undo FROM rule_run_actions
                 WHERE run_id = ?1 ORDER BY seq",
            )?;
            let rows = stmt.query_map([n], |r| {
                let action: String = r.get(2)?;
                let undo: Option<String> = r.get(3)?;
                Ok(RuleRunAction {
                    message_id: MessageId(r.get(0)?),
                    op_id: r.get::<_, Option<i64>>(1)?.map(OpId),
                    action: from_json(2, &action)?,
                    undo: undo.as_deref().map(|u| from_json(3, u)).transpose()?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Deletes finished runs started before `before` (Unix seconds).
    /// Returns how many were deleted.
    pub fn purge_rule_runs(&self, before: i64) -> Result<usize> {
        self.with_conn(|conn| {
            Ok(conn.execute(
                "DELETE FROM rule_runs WHERE state <> 'running' AND started_at < ?1",
                [before],
            )?)
        })
    }

    /// Queues new messages for the arrival rules (already queued ones are
    /// kept with their first time).
    pub fn queue_for_rules(&self, messages: &[MessageId], at: i64) -> Result<()> {
        self.with_tx(|tx| {
            let mut stmt = tx.prepare_cached(
                "INSERT OR IGNORE INTO rule_inbox (message_id, queued_at)
                 SELECT ?1, ?2 WHERE EXISTS (SELECT 1 FROM messages WHERE id = ?1)",
            )?;
            for id in messages {
                stmt.execute(params![id.as_str(), at])?;
            }
            Ok(())
        })
    }

    /// Up to `limit` messages waiting for the arrival rules, oldest first.
    pub fn rule_inbox(&self, limit: u32) -> Result<Vec<MessageId>> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare_cached(
                "SELECT message_id FROM rule_inbox ORDER BY queued_at, message_id LIMIT ?1",
            )?;
            let rows = stmt.query_map([limit], |r| Ok(MessageId(r.get(0)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Removes processed messages from the rule inbox.
    pub fn clear_rule_inbox(&self, messages: &[MessageId]) -> Result<()> {
        self.with_tx(|tx| {
            let mut stmt = tx.prepare_cached("DELETE FROM rule_inbox WHERE message_id = ?1")?;
            for id in messages {
                stmt.execute([id.as_str()])?;
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use nail_api::rules::FolderRef;
    use nail_api::{Account, AccountKind, Folder, FolderRole, MessageSummary};

    use super::*;
    use crate::queue::FlagChange;

    fn store_with_message() -> Store {
        let store = Store::open_in_memory().unwrap();
        store
            .upsert_account(&Account {
                id: "a1".into(),
                name: "A".into(),
                email: "a@example.org".into(),
                kind: AccountKind::Imap,
            })
            .unwrap();
        store
            .upsert_folder(&Folder {
                id: "inbox".into(),
                account_id: "a1".into(),
                parent_id: None,
                name: "Inbox".into(),
                path: "INBOX".into(),
                role: FolderRole::Inbox,
                total: 0,
                unread: 0,
            })
            .unwrap();
        let summary = MessageSummary {
            id: "m1".into(),
            folder_id: "inbox".into(),
            account_id: "a1".into(),
            subject: "Hi".into(),
            from: None,
            to: Vec::new(),
            date: "2026-09-29T10:00:00Z".into(),
            flags: nail_api::Flags::default(),
            has_attachments: false,
            preview: String::new(),
            thread_id: None,
            tags: Vec::new(),
            duplicates: Vec::new(),
            headers_only: false,
            gmail: None,
            exchange: None,
        };
        store
            .upsert_message(&crate::StoredMessage::new(summary))
            .unwrap();
        store
    }

    fn flag_undo() -> Operation {
        Operation::UpdateFlags {
            message_id: "m1".into(),
            change: FlagChange {
                flagged: Some(false),
                ..FlagChange::default()
            },
        }
    }

    #[test]
    fn runs_record_actions_and_undo_once() {
        let store = store_with_message();
        let rule = RuleId::from("r1");
        let run = store
            .start_rule_run(&rule, "Flag it", RuleRunCause::Manual, 100)
            .unwrap();
        assert_eq!(run.as_str(), "run-1");
        store
            .record_rule_action(
                &run,
                &RuleRunAction {
                    message_id: "m1".into(),
                    op_id: Some(OpId(5)),
                    action: RuleAction::Flag { flagged: true },
                    undo: Some(flag_undo()),
                },
            )
            .unwrap();
        store
            .record_rule_action(
                &run,
                &RuleRunAction {
                    message_id: "m1".into(),
                    op_id: None,
                    action: RuleAction::Stop,
                    undo: None,
                },
            )
            .unwrap();
        let running = store.get_rule_run(&run).unwrap();
        assert_eq!(running.state, RuleRunState::Running);
        assert!(!running.undoable);

        let done = store
            .finish_rule_run(&run, RuleRunState::Done, 1, None, 101)
            .unwrap();
        assert_eq!(done.op_ids, [OpId(5)]);
        assert!(done.undoable);
        assert_eq!((done.matched, done.finished_at), (1, Some(101)));
        let actions = store.rule_run_actions(&run).unwrap();
        assert_eq!(actions.len(), 2);
        assert_eq!(actions[0].undo, Some(flag_undo()));
        assert_eq!(actions[1].action, RuleAction::Stop);

        // Finished runs take no more actions and finish once.
        assert!(matches!(
            store.record_rule_action(&run, &actions[1]),
            Err(Error::Conflict(_))
        ));
        assert!(matches!(
            store.finish_rule_run(&run, RuleRunState::Done, 1, None, 102),
            Err(Error::Conflict(_))
        ));
        assert!(matches!(
            store.finish_rule_run(&run, RuleRunState::Undone, 1, None, 102),
            Err(Error::InvalidData(_))
        ));

        let undone = store.mark_rule_run_undone(&run).unwrap();
        assert_eq!(undone.state, RuleRunState::Undone);
        assert!(!undone.undoable);
        assert!(matches!(
            store.mark_rule_run_undone(&run),
            Err(Error::Conflict(_))
        ));
        assert!(
            store
                .get_rule_run(&"run-9".into())
                .unwrap_err()
                .is_not_found()
        );
    }

    fn move_back(message: &str) -> Operation {
        Operation::MoveMessage {
            message_id: message.into(),
            target_folder_id: "inbox".into(),
            source: None,
        }
    }

    #[test]
    fn undo_skips_stale_inverses_and_restores_the_rest() {
        let store = store_with_message();
        store
            .upsert_folder(&Folder {
                id: "archive".into(),
                account_id: "a1".into(),
                parent_id: None,
                name: "Archive".into(),
                path: "Archive".into(),
                role: FolderRole::Archive,
                total: 0,
                unread: 0,
            })
            .unwrap();
        let mut m2 = store.get_message(&"m1".into()).unwrap();
        m2.summary.id = "m2".into();
        m2.summary.folder_id = "archive".into();
        store.upsert_message(&m2).unwrap();
        // The run moved m1 and m2 to Archive and flagged m3. Since then the
        // user moved m1 back by hand and m3 was expunged.
        let run = store
            .start_rule_run(&"r1".into(), "Archive", RuleRunCause::Manual, 1)
            .unwrap();
        let archive = RuleAction::Archive;
        for (n, (message, undo)) in [
            ("m1", move_back("m1")),
            ("m2", move_back("m2")),
            (
                "m3",
                Operation::UpdateFlags {
                    message_id: "m3".into(),
                    change: FlagChange::default(),
                },
            ),
        ]
        .into_iter()
        .enumerate()
        {
            store
                .record_rule_action(
                    &run,
                    &RuleRunAction {
                        message_id: message.into(),
                        op_id: Some(OpId(i64::try_from(n).unwrap() + 1)),
                        action: archive.clone(),
                        undo: Some(undo),
                    },
                )
                .unwrap();
        }
        store
            .finish_rule_run(&run, RuleRunState::Done, 3, None, 2)
            .unwrap();

        let undo = store.undo_rule_run(&run).unwrap();
        assert_eq!(undo.skipped, 2);
        assert_eq!(undo.queued.len(), 1);
        assert_eq!(undo.queued[0].op.operation.message_id().as_str(), "m2");
        assert_eq!(undo.record.state, RuleRunState::Undone);
        assert_eq!(
            store.get_message(&"m2".into()).unwrap().summary.folder_id,
            nail_api::FolderId::from("inbox")
        );
        // Once only.
        assert!(matches!(store.undo_rule_run(&run), Err(Error::Conflict(_))));
        assert_eq!(store.list_ops(None).unwrap().len(), 1);
    }

    #[test]
    fn a_refused_undo_changes_nothing() {
        let store = store_with_message();
        let run = store
            .start_rule_run(&"r1".into(), "Move", RuleRunCause::Manual, 1)
            .unwrap();
        // Still running: not undoable, and nothing is queued or marked.
        store
            .record_rule_action(
                &run,
                &RuleRunAction {
                    message_id: "m1".into(),
                    op_id: Some(OpId(1)),
                    action: RuleAction::Flag { flagged: true },
                    undo: Some(flag_undo()),
                },
            )
            .unwrap();
        assert!(matches!(store.undo_rule_run(&run), Err(Error::Conflict(_))));
        assert_eq!(
            store.get_rule_run(&run).unwrap().state,
            RuleRunState::Running
        );
        assert!(store.list_ops(None).unwrap().is_empty());
        assert!(
            store
                .undo_rule_run(&"run-9".into())
                .unwrap_err()
                .is_not_found()
        );
    }

    #[test]
    fn runs_without_inverse_are_not_undoable() {
        let store = store_with_message();
        let rule = RuleId::from("r1");
        let run = store
            .start_rule_run(&rule, "Move", RuleRunCause::Arrival, 1)
            .unwrap();
        store
            .record_rule_action(
                &run,
                &RuleRunAction {
                    message_id: "m1".into(),
                    op_id: Some(OpId(1)),
                    action: RuleAction::Move {
                        folder: FolderRef::Role {
                            role: FolderRole::Archive,
                            account_id: None,
                        },
                    },
                    undo: None,
                },
            )
            .unwrap();
        let done = store
            .finish_rule_run(&run, RuleRunState::Done, 1, None, 2)
            .unwrap();
        assert!(!done.undoable);
        let failed_run = store
            .start_rule_run(&"r2".into(), "Other", RuleRunCause::Schedule, 3)
            .unwrap();
        store
            .finish_rule_run(&failed_run, RuleRunState::Failed, 0, Some("boom"), 4)
            .unwrap();

        let all = store.list_rule_runs(None, 10).unwrap();
        assert_eq!(
            all.iter().map(|r| r.id.as_str()).collect::<Vec<_>>(),
            ["run-2", "run-1"]
        );
        assert_eq!(all[0].message.as_deref(), Some("boom"));
        assert_eq!(store.list_rule_runs(Some(&rule), 10).unwrap().len(), 1);
        assert_eq!(store.purge_rule_runs(3).unwrap(), 1);
        assert_eq!(store.list_rule_runs(None, 10).unwrap().len(), 1);
    }

    #[test]
    fn rule_state_and_inbox() {
        let store = store_with_message();
        let rule = RuleId::from("r1");
        assert_eq!(store.rule_state(&rule).unwrap(), RuleState::default());
        let state = RuleState {
            last_run_at: Some(10),
            next_run_at: Some(70),
            run_count: 3,
        };
        store.set_rule_state(&rule, &state).unwrap();
        assert_eq!(store.rule_state(&rule).unwrap(), state);
        assert_eq!(
            store
                .retain_rule_states(std::slice::from_ref(&rule))
                .unwrap(),
            0
        );
        assert_eq!(store.retain_rule_states(&[]).unwrap(), 1);
        assert_eq!(store.rule_state(&rule).unwrap(), RuleState::default());

        store
            .queue_for_rules(&["m1".into(), "gone".into()], 5)
            .unwrap();
        store.queue_for_rules(&["m1".into()], 9).unwrap();
        assert_eq!(store.rule_inbox(10).unwrap(), [MessageId::from("m1")]);
        store.clear_rule_inbox(&["m1".into()]).unwrap();
        assert!(store.rule_inbox(10).unwrap().is_empty());
        // Deleting a message drops it from the inbox.
        store.queue_for_rules(&["m1".into()], 11).unwrap();
        store.delete_message(&"m1".into()).unwrap();
        assert!(store.rule_inbox(10).unwrap().is_empty());
    }
}
