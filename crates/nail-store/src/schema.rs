//! Numbered schema migrations.
//!
//! [`MIGRATIONS`]`[n]` upgrades a database from version `n` to `n + 1`. Each
//! migration runs in its own `IMMEDIATE` transaction together with the row
//! that records it in `schema_version`, so a crash leaves the database at
//! a whole version. Migrations are append-only: never edit a released one,
//! add a new one instead. The SQL of each released step is its own file in
//! `src/migrations/` (`vNN.sql`, embedded byte for byte).

use rusqlite::{Connection, Transaction, TransactionBehavior};

use crate::error::{Error, Result};

/// Schema v1: accounts, folders, messages and the operation queue.
const V1: &str = include_str!("migrations/v01.sql");

/// Schema v2 (M1a): threading headers and thread ids, tags, message size,
/// per-message operation index, drafts and the outbox. Identities and
/// virtual accounts are **not** stored: `accounts.kdl` is their only source
/// of truth (ADR 0012). `folders.highestmodseq` exists since v1.
const V2: &str = include_str!("migrations/v02.sql");

/// Schema v3 (M1a WP14): POP3 bookkeeping. POP3 has no folders or numeric
/// UIDs on the server; the core remembers which `UIDL`s it downloaded (and
/// when), which local message each became, and whether the user deleted
/// that local copy, for the leave-on-server policies.
const V3: &str = include_str!("migrations/v03.sql");

/// Schema v4 (M1b WP20, ADR 0022): rich drafts, collected addresses,
/// rule runs, MCP clients and saved searches.
///
/// * Drafts keep their full `nail-api` JSON in `payload`; the new columns
///   index what the core queries (mode, reply / forward context, the copy
///   in the server's Drafts folder), and `draft_attachments` lists the
///   blobs a draft references so blob garbage collection keeps them.
/// * Rules themselves live in `rules.kdl` (the source of truth, like
///   `accounts.kdl`); the store keeps their runtime state, the run history
///   with every queued action and its inverse (undo as one unit), and the
///   messages waiting for arrival rules.
/// * MCP clients are stored with a SHA-256 hash of their token only.
const V4: &str = include_str!("migrations/v04.sql");

/// Schema v5 (M2a WP30, ADR 0029): the keyring, Autocrypt peer state,
/// trust decisions, encryption-at-rest state, cached message security and
/// a key/value settings table.
///
/// * `crypto_keys` holds OpenPGP keys and S/MIME certificates: metadata
///   the UI lists, the public part (binary OpenPGP packets / DER) and, for
///   own keys, the **encrypted** secret part (OpenPGP S2K-protected or
///   wrapped with a keychain key; PKCS#8 for S/MIME) or a reference to
///   where it lives (keychain entry, Windows store thumbprint). Nothing in
///   this database is ever a plain secret key.
/// * `crypto_key_emails` indexes keys by address for recipient lookup.
/// * `crypto_trust` keeps the user's decisions apart from the key rows, so
///   deleting and re-importing a key keeps them.
/// * `autocrypt_peers` is Autocrypt Level 1 peer state, per own account.
/// * `vault_state` is a single row describing encryption at rest and a
///   running migration (for crash-safe resume).
/// * `message_security` caches what reading a message found (security
///   status and trusted `Authentication-Results`, never decrypted
///   content), for list badges and rule conditions.
/// * `app_settings` holds small JSON settings such as
///   [`nail_api::security::SecuritySettings`].
const V5: &str = include_str!("migrations/v05.sql");

/// Schema v6 (M2b WP40, ADR 0036): the remote UI and remote content.
///
/// **Only hashed or referenced secrets.** Passwords are Argon2id PHC
/// strings (a `CHECK` refuses anything else); TOTP secrets live in the OS
/// keychain and only their `keychain:` reference is stored; device
/// secrets, session tokens and pairing codes are stored as SHA-256 (hex).
/// Passkey rows hold the public credential (public key, counter).
///
/// * `remote_users` — login name, credentials (as above), grant ceiling
///   (`nail_api::remote::RemoteGrant` JSON), disabled / lockout state and
///   the last accepted TOTP step (replay protection).
/// * `remote_devices` — paired browsers with the hash of their device
///   cookie and their grant.
/// * `remote_passkeys` — WebAuthn credentials per user.
/// * `remote_sessions` — sessions with the hash of the current token and,
///   for a short grace period after a rotation, of the previous one.
/// * `remote_pairing_codes` — one-time codes (hash), single use.
/// * `remote_throttle` — failed-login counters per address and per login
///   name, so backoff and lockouts survive a restart.
/// * `remote_audit` — the remote-UI audit log.
/// * `remote_content_allow` — remote-content allow-lists (per message,
///   sender, domain). Message entries have no foreign key (ids are
///   strings, also of virtual messages); the core removes stale ones.
const V6: &str = include_str!("migrations/v06.sql");

/// Schema v7 (M2c WP50, ADR 0043): sync policies, snooze, the junk
/// classifier, notification overrides and the Sieve state.
///
/// * `messages.body_state` — where the body is (`synced`, `headers-only`,
///   `on-demand`; `nail_api::sync::BodyState`). Existing rows are `synced`.
///   The policy itself stays in `accounts.kdl`.
/// * `snoozed_messages` — wake times of snoozed messages (the message sits
///   in the hidden Snoozed folder with the `$Snoozed` keyword). No foreign
///   key to `messages`: a resync may give the message a new row; the core
///   rebinds it by `rfc_message_id` or drops the entry.
/// * `junk_settings`, `junk_tokens`, `junk_training`, `junk_verdicts` — the
///   per-account classifier. Tokens are stored **hashed** (64-bit prefix of
///   SHA-256, [`crate::hash_junk_token`]) so the table does not spell out
///   words of the mail; training is recorded per message (by `Message-ID`)
///   so re-marking a message moves it between classes instead of counting
///   it twice.
/// * `notify_overrides` — per-account / per-folder notification overrides
///   (removed with their account or folder by triggers); the global
///   notification settings live in `app_settings`.
/// * `sieve_state` — per account: hash and time of the uploaded script,
///   the server's extensions and other scripts, the vacation settings and
///   the last error.
const V7: &str = include_str!("migrations/v07.sql");

/// Schema v8 (M2c review fixes, ADR 0049):
///
/// * `sieve_state.location` — where the uploaded script lives (JSON:
///   host, port, TLS mode, script name), so switching ManageSieve off or
///   changing the host, port or script name removes Nail's script from the
///   place it was uploaded to, not from the current settings' place.
/// * `sieve_state.server_rules` — the rules the uploaded (and still active)
///   script runs (JSON array of rule ids). Arrival rules skip them for the
///   account's mail; it survives restarts and failed uploads (the script
///   stays active on the server) and is cleared only when the script is
///   removed.
/// * `folder_marks` — folders Nail created for itself (`snoozed`: the
///   hidden Snoozed folder). A user folder that happens to be called
///   `Snoozed` is not marked and stays visible. Removed with the folder.
///   Existing top-level `Snoozed` folders holding snoozed mail are marked.
const V8: &str = include_str!("migrations/v08.sql");

/// Schema v9 (M2d, ADR 0050): the plug-in preview.
///
/// * `plugins` — installed plug-ins: the manifest as installed (JSON,
///   texts unresolved), where it came from (`file`, `url`, `dev`), the
///   package hash, whether it is enabled, its settings (JSON object of the
///   values the user changed), its storage quota and the last error
///   (JSON `ApiError`) that stopped it.
/// * `plugin_grants` — granted permissions per plug-in (`accounts`: JSON
///   array for mail permissions limited to accounts; `NULL` = all).
/// * `plugin_storage` — the per-plug-in key-value storage (`nail.storage`).
///   Values are JSON, **sealed at rest** when encryption at rest is on
///   (`SEALED_COLUMNS`); `size` = key + plain JSON bytes, summed against the
///   plug-in's quota. Rows are addressed by `id` for sealing.
/// * `plugin_logs` — log lines per plug-in (bounded, the oldest go first);
///   the message is sealed at rest (it may quote mail).
///
/// Everything of a plug-in goes with it (`ON DELETE CASCADE`).
const V9: &str = include_str!("migrations/v09.sql");

/// Schema v10 (M3a, ADR 0058): calendars, contacts and tasks.
///
/// * `pim_sources` — one row per source: the built-in `local` one
///   (inserted here) and every configured source the core has seen.
///   Settings stay in `accounts.kdl`; the row holds discovery results and
///   sync state.
/// * `pim_collections` — calendars / task lists (`calendar`) and address
///   books (`address-book`) of a source: the user's settings (JSON), the
///   server URL and the sync tokens (CalDAV / CardDAV `getctag`,
///   RFC 6578 sync-token, ICS `ETag` / `Last-Modified`). `role` marks the
///   local "Collected" book.
/// * `pim_objects` — every iCalendar / vCard resource as received or last
///   written (lossless; **sealed at rest**), its server URL and ETag, and
///   its sync state; on a conflict the server's version wins and the local
///   one is kept in `conflict_raw` (sealed).
/// * `pim_events`, `pim_tasks`, `pim_contacts` — the parsed items (JSON of
///   the `nail_api` types, sealed) with the plain columns lists, sorting
///   and range queries need (titles and names stay readable, like
///   subjects). Groups are `pim_contacts` rows of kind `group` with
///   `pim_group_members`; `pim_contact_emails` serves autocomplete and
///   duplicate detection; `pim_contact_links` holds merged persons.
/// * `pim_event_instances` — the instance cache: expanded occurrences in
///   Unix seconds; dates and floating times are stored as UTC wall time
///   (`floating` = 1) and placed in the viewer's zone when read
///   (range queries widen by 14 hours for them). `pim_instance_windows`
///   says how far each event has been expanded.
/// * `pim_reminders` — alarm occurrences with their state (pending, fired,
///   snoozed, dismissed); removed with their event or task by triggers.
/// * `pim_ops` — the PIM operation queue (like `op_queue`, per source).
///
/// Everything of a source goes with it (`ON DELETE CASCADE`).
const V10: &str = include_str!("migrations/v10.sql");

/// Schema v11 (M3b, ADR 0065): scheduling state and Gmail accounts.
///
/// * `messages.remote_id` — the message's id on an API server (Gmail; M3c
///   Graph / EWS), unique per account and kept across moves (API messages
///   are stored once, their folder follows their labels).
/// * `itip_state` — per event (`UID` + recurrence id, `''` = the whole
///   event) and address (organizer or attendee, lowercase): the newest
///   `SEQUENCE` / `DTSTAMP` applied and the answer (`PARTSTAT`), so older
///   iTIP messages are recognised as outdated.
/// * `itip_messages` — every received iTIP message and what was done
///   with it (`new`, `applied`, `outdated`, `ignored`, `failed`).
/// * `itip_sent` — every iTIP message Nail sent (or had the CalDAV server
///   send): replies, requests, cancels, counters, refreshes.
/// * `gmail_sync` — per Gmail account: the last applied `historyId` and an
///   interrupted full sync's page token.
/// * `gmail_labels` — the account's labels (id ↔ name ↔ Nail tag / folder,
///   colours, visibility, local sync state).
/// * `gmail_messages` — Gmail's thread id, label ids and category of each
///   stored Gmail message.
/// * `gmail_label_ops` — the label operation queue (create / update /
///   delete), per account.
const V11: &str = include_str!("migrations/v11.sql");

/// Schema v12 (M3c, ADR 0072): Exchange accounts.
///
/// * `pim_sources` is rebuilt (SQLite cannot change a `CHECK`): `kind`
///   also takes `exchange`, and a new `account_id` ties an Exchange
///   account's PIM source to the account (deleting the account deletes
///   the source and its collections). Runs with foreign keys off
///   ([`Migration::foreign_keys_off`]), checked before commit.
/// * `exchange_sync` — per account: engine, folder-hierarchy cursor per
///   mailbox is in `exchange_mailbox_sync`; server version, EWS streaming
///   subscription, throttling, the rules state, last sync.
/// * `exchange_mailbox_sync` — the folder-hierarchy cursor (Graph
///   `mailFolders/delta` link, EWS `SyncFolderHierarchy` state) per
///   mailbox (`''` = the account's own).
/// * `exchange_folders` — per mail folder: mailbox, remote id, change key,
///   parent, well-known name and the items cursor (Graph
///   `messages/delta` link, EWS `SyncFolderItems` state) plus an
///   interrupted first sync.
/// * `exchange_items` — per message: mailbox, change key, conversation id,
///   Focused Inbox classification (the remote id is `messages.remote_id`).
/// * `exchange_pim_collections` / `exchange_pim_items` — the same for
///   calendars, contact folders and task lists (cursor per collection;
///   remote id + change key per item).
/// * `exchange_mailboxes` — shared / delegate mailboxes (what is shown,
///   automapped / hidden, reachability, root folder).
/// * `exchange_categories` — the master category list (id ↔ name = Nail
///   tag, colour, local sync state).
/// * `exchange_automatic_replies` — the out-of-office settings (sealed).
/// * `exchange_rules` — the server's inbox rules and which Nail rule each
///   was placed from.
/// * `exchange_rooms` — cached room lists and rooms.
/// * `exchange_ops` — the Exchange operation queue (categories, automatic
///   replies, inbox rules, Focused Inbox); payloads sealed.
const V12: &str = include_str!("migrations/v12.sql");

/// Schema v13 (M3c review fixes, ADR 0078): resumable Exchange listings,
/// moves seen from other clients, meeting answers.
///
/// * `exchange_pim_collections.full_sync_page` — a full listing of a
///   calendar / contact folder / task list in progress (the page to go on
///   with), like `exchange_folders.full_sync_page` for mail folders.
/// * `exchange_listed` — the server ids a full listing of a mail folder or
///   PIM collection (`scope`) has seen so far; the merge after the last
///   page removes what the server no longer lists, also when the listing
///   took several passes.
/// * `exchange_removed` — messages a folder's changes reported as removed,
///   kept until every folder of the mailbox was synced: a message another
///   client moved shows up in its new folder and is moved locally instead
///   of being downloaded again.
/// * `exchange_meeting_answers` — per meeting the user was invited to: the
///   own answer as last synced, and an answer queued from Nail with its
///   comment (sealed) and whether the organizer is told.
const V13: &str = include_str!("migrations/v13.sql");

/// Schema v14 (M4a, ADR 0079): writes by MCP clients and plug-ins.
///
/// * `confirmations` — writes waiting for (or decided by) the user: the
///   actor (MCP client or plug-in, with the MCP client a plug-in serves),
///   the action kind and operation, what the confirmation center shows
///   (`view`: summary, targets, before / after, accounts — **sealed**:
///   subjects and addresses) and what to run once approved (`payload`,
///   the requester's own JSON — **sealed**), the caller's idempotency key,
///   state and decision. Decided rows are kept as history (pruned to
///   `nail_api::agents::CONFIRMATION_HISTORY`).
/// * `action_policies` — "always allow" / "ask" per actor and kind, only
///   where the user changed the default; `allow` is refused for
///   destructive kinds (a `CHECK`, besides the API's own check). Rows of a
///   removed client or plug-in are deleted with it by the core (actors are
///   not foreign keys: MCP clients are rows, plug-ins ids).
/// * `mcp_clients.rate_limits` — the client's `McpRateLimits` (JSON; NULL =
///   the defaults); `mcp_idempotency` — results of write tool calls by
///   idempotency key (**sealed**), kept for a day.
/// * `plugin_jobs` — per background job: last / next run, outcome, counts;
///   `plugin_hook_stats` — per hook: calls, failures, time-outs, vetoes,
///   time; `plugin_cpu` — CPU time per plug-in and minute for the hourly
///   quota. All go with their plug-in.
const V14: &str = include_str!("migrations/v14.sql");

/// Schema v15 (M4c, ADR 0094): smartcard keys and server threads.
///
/// * `crypto_keys` is rebuilt so `source` and `secret_storage` may be
///   `'smartcard'` (a key on a PKCS#11 token: `secret_ref` holds the JSON of
///   its `nail_api::smartcard::SmartcardKeyRef`, `secret_data` is NULL).
///   `crypto_key_emails` references it, so the step runs with foreign keys
///   off ([`Migration::rebuild`]).
/// * `messages.remote_thread_id` — the server's thread / conversation id
///   (Gmail `threadId`, Graph / EWS conversation id); messages sharing one
///   share a Nail thread (`thread_refs` key `remote-thread:<account>:<id>`,
///   `Store::set_message_remote_thread_id`).
const V15: &str = include_str!("migrations/v15.sql");

/// One schema step: SQL plus an optional Rust backfill that runs in the same
/// transaction (for data SQLite cannot compute, e.g. normalized subjects).
#[derive(Clone, Copy)]
pub(crate) struct Migration {
    pub sql: &'static str,
    pub backfill: Option<fn(&Transaction<'_>) -> Result<()>>,
    /// The step rebuilds a table other tables reference (SQLite's
    /// "12-step" table change): foreign keys are switched off around its
    /// transaction (a `DROP TABLE` would otherwise cascade) and
    /// `PRAGMA foreign_key_check` must come back empty before the commit.
    pub foreign_keys_off: bool,
}

/// Schema v16 (P2 WP201, ADR 0149): conversations.
///
/// * `messages_folder_thread` — a folder's messages by thread, newest last:
///   a conversation list groups a folder by thread, counts its threads and
///   joins the Sent folders' messages of the listed threads from this index
///   alone (no row reads; `thread::activity::GROUPING_PAGE_SQL`).
const V16: &str = include_str!("migrations/v16.sql");

/// Schema v17 (P3 contract, ADR 0164): conversation speed and conversation
/// actions.
///
/// * `thread_activity` — one row per (folder, thread) with the thread's
///   newest message in the folder (`folder_last`), the row's order key
///   (`last_date`: also the user's own messages of the thread in the Sent
///   folders of the folder's account, P2's Sent join) and the thread's
///   messages in the folder (`message_count`, copies not collapsed). Drafts
///   folders have no rows (their messages are rows of their own). A page of
///   a folder's conversations is an index range of
///   `thread_activity_order` (`thread::activity`); WP-perf keeps the table
///   current (schema v18) and switches `ListThreads` to it.
/// * `mail_batches` / `mail_batch_ops` — a mail action's queued operations
///   with their inverses: undo as one unit (`UndoMailAction`), like a rule
///   run's actions; WP-actions-core writes and reads them.
const V17: &str = include_str!("migrations/v17.sql");

impl Migration {
    pub(crate) const fn sql(sql: &'static str) -> Self {
        Self {
            sql,
            backfill: None,
            foreign_keys_off: false,
        }
    }

    /// A step that rebuilds a referenced table (see
    /// [`Self::foreign_keys_off`]).
    pub(crate) const fn rebuild(sql: &'static str) -> Self {
        Self {
            sql,
            backfill: None,
            foreign_keys_off: true,
        }
    }
}

/// All migrations; index `n` upgrades version `n` to `n + 1`.
pub(crate) const MIGRATIONS: &[Migration] = &[
    Migration::sql(V1),
    Migration {
        sql: V2,
        backfill: Some(crate::thread::backfill_v2),
        foreign_keys_off: false,
    },
    Migration::sql(V3),
    Migration {
        sql: V4,
        backfill: Some(crate::compose::backfill_v4),
        foreign_keys_off: false,
    },
    Migration::sql(V5),
    Migration::sql(V6),
    Migration::sql(V7),
    Migration::sql(V8),
    Migration::sql(V9),
    Migration::sql(V10),
    Migration::sql(V11),
    Migration::rebuild(V12),
    Migration::sql(V13),
    Migration::sql(V14),
    Migration::rebuild(V15),
    Migration::sql(V16),
    Migration::sql(V17),
    // Schema v18 (P3 WP-perf): triggers keep `thread_activity` current, and
    // the table is rebuilt once (`thread::activity::MAINTENANCE_SQL`).
    Migration::sql(crate::thread::activity::MAINTENANCE_SQL),
];

/// The schema version this build creates and understands.
#[expect(
    clippy::cast_possible_truncation,
    reason = "a few dozen migrations; the assertion below checks the bound at compile time"
)]
pub const SCHEMA_VERSION: u32 = MIGRATIONS.len() as u32;
const _: () = assert!(MIGRATIONS.len() < 1 << 16);

const CREATE_VERSION_TABLE: &str = "CREATE TABLE IF NOT EXISTS schema_version (
    version    INTEGER PRIMARY KEY NOT NULL,
    applied_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
)";

/// The version recorded in the database (0 for an empty database).
pub(crate) fn current_version(conn: &Connection) -> Result<u32> {
    conn.execute_batch(CREATE_VERSION_TABLE)?;
    Ok(conn.query_row(
        "SELECT COALESCE(MAX(version), 0) FROM schema_version",
        [],
        |row| row.get(0),
    )?)
}

/// Brings the database up to [`SCHEMA_VERSION`] using [`MIGRATIONS`].
/// Returns the resulting version.
pub(crate) fn migrate(conn: &mut Connection) -> Result<u32> {
    migrate_with(conn, MIGRATIONS)
}

/// Brings the database up to `target` only; creates old-version databases
/// for upgrade tests.
#[cfg(test)]
pub(crate) fn migrate_to_version(conn: &mut Connection, target: u32) -> Result<u32> {
    let target = (target as usize).min(MIGRATIONS.len());
    migrate_with(conn, &MIGRATIONS[..target])
}

pub(crate) fn migrate_with(conn: &mut Connection, migrations: &[Migration]) -> Result<u32> {
    let supported = u32::try_from(migrations.len()).unwrap_or(u32::MAX);
    loop {
        // A step that rebuilds a referenced table runs with foreign keys
        // off; the pragma is a no-op inside a transaction, so it is set
        // before (and restored after) the step's transaction.
        let peek = current_version(conn)?;
        let rebuild = migrations
            .get(peek as usize)
            .is_some_and(|m| m.foreign_keys_off);
        let restore_fks = if rebuild {
            let on: bool = conn.query_row("PRAGMA foreign_keys", [], |r| r.get(0))?;
            conn.pragma_update(None, "foreign_keys", false)?;
            on
        } else {
            false
        };
        let step_result = migrate_step(conn, migrations, supported);
        if restore_fks {
            conn.pragma_update(None, "foreign_keys", true)?;
        }
        if let Some(version) = step_result? {
            return Ok(version);
        }
    }
}

/// Applies the next migration in its own transaction; `Some(version)` when
/// the database is already at `supported`.
fn migrate_step(
    conn: &mut Connection,
    migrations: &[Migration],
    supported: u32,
) -> Result<Option<u32>> {
    {
        // Re-read the version inside the write transaction so two processes
        // opening the same fresh database cannot both apply a migration.
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version = current_version(&tx)?;
        if version > supported {
            return Err(Error::SchemaTooNew {
                found: version,
                supported,
            });
        }
        if version == supported {
            tx.commit()?;
            return Ok(Some(version));
        }
        let next = version + 1;
        tracing::info!(from = version, to = next, "migrating store schema");
        let step = migrations[version as usize];
        tx.execute_batch(step.sql)?;
        if let Some(backfill) = step.backfill {
            backfill(&tx)?;
        }
        if step.foreign_keys_off {
            let broken: i64 =
                tx.query_row("SELECT COUNT(*) FROM pragma_foreign_key_check", [], |r| {
                    r.get(0)
                })?;
            if broken > 0 {
                return Err(Error::InvalidData(format!(
                    "schema step {next} left {broken} broken references"
                )));
            }
        }
        tx.execute("INSERT INTO schema_version (version) VALUES (?1)", [next])?;
        tx.commit()?;
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tables(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' ORDER BY name")
            .unwrap();
        stmt.query_map([], |row| row.get(0))
            .unwrap()
            .collect::<Result<_, _>>()
            .unwrap()
    }

    #[test]
    fn migrates_empty_database_to_latest() {
        let mut conn = Connection::open_in_memory().unwrap();
        assert_eq!(current_version(&conn).unwrap(), 0);
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
        assert_eq!(current_version(&conn).unwrap(), SCHEMA_VERSION);
        assert_eq!(
            tables(&conn),
            [
                "accounts",
                "action_policies",
                "app_settings",
                "autocrypt_peers",
                "collected_addresses",
                "confirmations",
                "crypto_key_emails",
                "crypto_keys",
                "crypto_trust",
                "draft_attachments",
                "drafts",
                "exchange_automatic_replies",
                "exchange_categories",
                "exchange_folders",
                "exchange_items",
                "exchange_listed",
                "exchange_mailbox_sync",
                "exchange_mailboxes",
                "exchange_meeting_answers",
                "exchange_ops",
                "exchange_pim_collections",
                "exchange_pim_items",
                "exchange_removed",
                "exchange_rooms",
                "exchange_rules",
                "exchange_sync",
                "folder_marks",
                "folders",
                "gmail_label_ops",
                "gmail_labels",
                "gmail_messages",
                "gmail_sync",
                "itip_messages",
                "itip_sent",
                "itip_state",
                "junk_settings",
                "junk_tokens",
                "junk_training",
                "junk_verdicts",
                "mail_batch_ops",
                "mail_batches",
                "mcp_audit",
                "mcp_clients",
                "mcp_grants",
                "mcp_idempotency",
                "message_security",
                "message_tags",
                "messages",
                "notify_overrides",
                "op_queue",
                "outbox",
                "pim_collections",
                "pim_contact_emails",
                "pim_contact_links",
                "pim_contacts",
                "pim_event_instances",
                "pim_events",
                "pim_group_members",
                "pim_instance_windows",
                "pim_objects",
                "pim_ops",
                "pim_reminders",
                "pim_sources",
                "pim_tasks",
                "plugin_cpu",
                "plugin_grants",
                "plugin_hook_stats",
                "plugin_jobs",
                "plugin_logs",
                "plugin_storage",
                "plugins",
                "pop3_uids",
                "remote_audit",
                "remote_content_allow",
                "remote_devices",
                "remote_pairing_codes",
                "remote_passkeys",
                "remote_sessions",
                "remote_throttle",
                "remote_users",
                "rule_inbox",
                "rule_run_actions",
                "rule_runs",
                "rule_state",
                "saved_searches",
                "schema_version",
                "sieve_state",
                "snoozed_messages",
                "thread_activity",
                "thread_refs",
                "vault_state"
            ]
        );
    }

    #[test]
    fn migrate_is_idempotent() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        migrate(&mut conn).unwrap();
        let rows: u32 = conn
            .query_row("SELECT COUNT(*) FROM schema_version", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rows, SCHEMA_VERSION);
    }

    #[test]
    fn stops_at_the_requested_version() {
        let mut conn = Connection::open_in_memory().unwrap();
        assert_eq!(migrate_to_version(&mut conn, 1).unwrap(), 1);
        assert!(!tables(&conn).contains(&"thread_refs".to_owned()));
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
        assert!(tables(&conn).contains(&"thread_refs".to_owned()));
    }

    #[test]
    fn applies_only_missing_migrations_in_order() {
        let mut conn = Connection::open_in_memory().unwrap();
        let first = [Migration::sql("CREATE TABLE a (x INTEGER)")];
        assert_eq!(migrate_with(&mut conn, &first).unwrap(), 1);
        let both = [
            Migration::sql("CREATE TABLE a (x INTEGER)"), // would fail if re-applied
            Migration::sql("ALTER TABLE a ADD COLUMN y INTEGER; INSERT INTO a VALUES (1, 2)"),
        ];
        assert_eq!(migrate_with(&mut conn, &both).unwrap(), 2);
        let y: i64 = conn.query_row("SELECT y FROM a", [], |r| r.get(0)).unwrap();
        assert_eq!(y, 2);
    }

    #[test]
    fn backfill_runs_in_the_migration_transaction() {
        fn fill(tx: &Transaction<'_>) -> Result<()> {
            tx.execute("INSERT INTO a VALUES (7)", [])?;
            Ok(())
        }
        fn broken(_: &Transaction<'_>) -> Result<()> {
            Err(Error::InvalidData("backfill failed".into()))
        }
        let mut conn = Connection::open_in_memory().unwrap();
        let steps = [Migration {
            sql: "CREATE TABLE a (x INTEGER)",
            backfill: Some(fill),
            foreign_keys_off: false,
        }];
        migrate_with(&mut conn, &steps).unwrap();
        let x: i64 = conn.query_row("SELECT x FROM a", [], |r| r.get(0)).unwrap();
        assert_eq!(x, 7);

        let mut conn = Connection::open_in_memory().unwrap();
        let steps = [Migration {
            sql: "CREATE TABLE b (x INTEGER)",
            backfill: Some(broken),
            foreign_keys_off: false,
        }];
        assert!(migrate_with(&mut conn, &steps).is_err());
        assert_eq!(current_version(&conn).unwrap(), 0);
        assert!(!tables(&conn).contains(&"b".to_owned()));
    }

    #[test]
    fn failed_migration_rolls_back_whole_step() {
        let mut conn = Connection::open_in_memory().unwrap();
        let broken = [Migration::sql(
            "CREATE TABLE ok (x INTEGER); CREATE TABLE broken (;",
        )];
        assert!(migrate_with(&mut conn, &broken).is_err());
        assert_eq!(current_version(&conn).unwrap(), 0);
        assert!(!tables(&conn).contains(&"ok".to_owned()));
    }

    #[test]
    fn v4_upgrades_m1a_drafts() {
        // A database as M1a (schema v2 / v3) left it: a plain reply draft
        // with `in_reply_to` and a new one, one of them in the outbox.
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        assert_eq!(migrate_to_version(&mut conn, 3).unwrap(), 3);
        conn.execute_batch(
            r#"
            INSERT INTO accounts VALUES ('a1', 'A', 'a@example.org', 'imap');
            INSERT INTO drafts (identity_id, payload) VALUES ('ada@example.org',
                '{"identity_id":"ada@example.org","to":[],"cc":[],"bcc":[],"subject":"Re: Hi",
                  "mode":"plain","text":"> hi","html":null,"markdown":null,"attachments":[],
                  "in_reply_to":"m7"}');
            INSERT INTO drafts (identity_id, payload) VALUES ('ada@example.org',
                '{"identity_id":"ada@example.org","subject":"New","text":"x","in_reply_to":null}');
            INSERT INTO outbox (draft_id, account_id, identity_id, send_at) VALUES (2, 'a1', 'ada@example.org', 0);
            "#,
        )
        .unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);

        let rows: Vec<(String, String, Option<String>, Option<String>)> = {
            let mut stmt = conn
                .prepare("SELECT payload, mode, context_mode, context_message_id FROM drafts ORDER BY id")
                .unwrap();
            stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))
                .unwrap()
                .collect::<Result<_, _>>()
                .unwrap()
        };
        let reply: nail_api::Draft = serde_json::from_str(&rows[0].0).unwrap();
        assert_eq!(
            reply.context,
            Some(nail_api::DraftContext {
                mode: nail_api::DraftContextMode::Reply,
                message_id: "m7".into(),
            })
        );
        assert!(!rows[0].0.contains("in_reply_to"));
        assert_eq!(
            (
                rows[0].1.as_str(),
                rows[0].2.as_deref(),
                rows[0].3.as_deref()
            ),
            ("plain", Some("reply"), Some("m7"))
        );
        let new: nail_api::Draft = serde_json::from_str(&rows[1].0).unwrap();
        assert_eq!((new.subject.as_str(), new.context), ("New", None));
        assert_eq!(rows[1].2, None);
        let outbox: i64 = conn
            .query_row("SELECT COUNT(*) FROM outbox", [], |r| r.get(0))
            .unwrap();
        assert_eq!(outbox, 1);
    }

    #[test]
    fn v7_keeps_existing_messages_synced() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate_to_version(&mut conn, 6).unwrap();
        conn.execute_batch(
            "INSERT INTO accounts (id, name, email, kind) VALUES ('a', 'A', 'a@x', 'imap');
             INSERT INTO folders (id, account_id, name, path) VALUES ('f', 'a', 'INBOX', 'INBOX');
             INSERT INTO messages (id, account_id, folder_id, date) VALUES ('m', 'a', 'f', 'x');",
        )
        .unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
        let state: String = conn
            .query_row("SELECT body_state FROM messages WHERE id = 'm'", [], |r| {
                r.get(0)
            })
            .unwrap();
        assert_eq!(state, "synced");
        assert!(
            conn.execute("UPDATE messages SET body_state = 'bogus'", [])
                .is_err()
        );
        assert!(
            conn.execute(
                "INSERT INTO junk_tokens (account_id, token) VALUES ('a', 'plain-word')",
                []
            )
            .is_err()
        );
    }

    #[test]
    fn v8_marks_snoozed_folders_holding_snoozed_mail_only() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate_to_version(&mut conn, 7).unwrap();
        conn.execute_batch(
            "INSERT INTO accounts (id, name, email, kind) VALUES ('a', 'A', 'a@x', 'imap');
             INSERT INTO accounts (id, name, email, kind) VALUES ('b', 'B', 'b@x', 'imap');
             INSERT INTO folders (id, account_id, name, path) VALUES ('fa', 'a', 'Snoozed', 'Snoozed');
             INSERT INTO folders (id, account_id, name, path) VALUES ('fb', 'b', 'Snoozed', 'Snoozed');
             INSERT INTO messages (id, account_id, folder_id, date) VALUES ('m', 'a', 'fa', 'x');
             INSERT INTO messages (id, account_id, folder_id, date) VALUES ('n', 'b', 'fb', 'x');
             INSERT INTO snoozed_messages (message_id, account_id, until, wake_folder_id, snoozed_at)
                 VALUES ('m', 'a', 10, 'fa', 1);
             INSERT INTO sieve_state (account_id, script_hash) VALUES ('a', NULL);",
        )
        .unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
        let marked: Vec<String> = conn
            .prepare("SELECT folder_id FROM folder_marks")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(marked, ["fa"], "the other Snoozed folder is the user's");
        let rules: String = conn
            .query_row("SELECT server_rules FROM sieve_state", [], |r| r.get(0))
            .unwrap();
        assert_eq!(rules, "[]");
    }

    /// A Gmail account `g` with its inbox `fi` and one message `m`.
    const GMAIL_MESSAGE: &str = "
        INSERT INTO accounts (id, name, email, kind) VALUES ('g', 'G', 'g@x', 'gmail');
        INSERT INTO folders (id, account_id, name, path) VALUES ('fi', 'g', 'Inbox', 'INBOX');
        INSERT INTO messages (id, account_id, folder_id, date) VALUES ('m', 'g', 'fi', 'x');";

    #[test]
    fn v11_adds_remote_ids_scheduling_and_gmail_tables() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrate_to_version(&mut conn, 10).unwrap();
        conn.execute_batch(GMAIL_MESSAGE).unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
        conn.execute_batch(
            "UPDATE messages SET remote_id = '18c0' WHERE id = 'm';
             INSERT INTO gmail_messages (message_id, account_id, gmail_thread_id, label_ids, category)
                 VALUES ('m', 'g', 't1', '[\"INBOX\"]', 'social');
             INSERT INTO gmail_labels (account_id, label_id, name, kind, tag)
                 VALUES ('g', 'Label_1', 'Work', 'user', 'Work');
             INSERT INTO itip_messages (message_id, account_id, method, uid)
                 VALUES ('m', 'g', 'request', 'uid-1');",
        )
        .unwrap();
        // One remote id per account; categories and states are checked.
        assert!(
            conn.execute(
                "INSERT INTO messages (id, account_id, folder_id, date, remote_id)
                 VALUES ('n', 'g', 'fi', 'x', '18c0')",
                []
            )
            .is_err()
        );
        assert!(
            conn.execute("UPDATE gmail_messages SET category = 'spam'", [])
                .is_err()
        );
        assert!(
            conn.execute("UPDATE itip_messages SET state = 'bogus'", [])
                .is_err()
        );
        // Deleting the message removes its Gmail and iTIP rows; deleting
        // the account removes the labels.
        conn.execute("DELETE FROM messages WHERE id = 'm'", [])
            .unwrap();
        let left: i64 = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM gmail_messages) + (SELECT COUNT(*) FROM itip_messages)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
        conn.execute("DELETE FROM accounts WHERE id = 'g'", [])
            .unwrap();
        let labels: i64 = conn
            .query_row("SELECT COUNT(*) FROM gmail_labels", [], |r| r.get(0))
            .unwrap();
        assert_eq!(labels, 0);
    }

    #[test]
    fn v14_keeps_mcp_clients_and_adds_agent_tables() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrate_to_version(&mut conn, 13).unwrap();
        conn.execute_batch(
            "INSERT INTO mcp_clients (name, token_hash, token_hint, created_at)
                 VALUES ('agent', 'h', 'hint', 1);",
        )
        .unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
        let limits: Option<String> = conn
            .query_row("SELECT rate_limits FROM mcp_clients", [], |r| r.get(0))
            .unwrap();
        assert_eq!(limits, None);
        // Idempotency entries go with their client; confirmations accept
        // only known kinds and states.
        conn.execute(
            "INSERT INTO mcp_idempotency (client_id, key, tool, result, created_at)
             VALUES (1, 'k', 'send_message', '{}', 1)",
            [],
        )
        .unwrap();
        conn.execute("DELETE FROM mcp_clients", []).unwrap();
        let left: i64 = conn
            .query_row("SELECT COUNT(*) FROM mcp_idempotency", [], |r| r.get(0))
            .unwrap();
        assert_eq!(left, 0);
        assert!(
            conn.execute(
                "INSERT INTO confirmations (actor_kind, actor_id, actor_name, kind, operation,
                     view, created_at, expires_at)
                 VALUES ('plugin', 'x', 'X', 'teleport', 'op', '{}', 1, 2)",
                []
            )
            .is_err()
        );
    }

    #[test]
    fn v12_rebuilds_pim_sources_and_adds_exchange_tables() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrate_to_version(&mut conn, 11).unwrap();
        conn.execute_batch(
            "INSERT INTO accounts (id, name, email, kind) VALUES ('x', 'X', 'x@x', 'exchange');
             INSERT INTO folders (id, account_id, name, path) VALUES ('fi', 'x', 'Inbox', 'Inbox');
             INSERT INTO messages (id, account_id, folder_id, date) VALUES ('m', 'x', 'fi', 'x');
             INSERT INTO pim_sources (id, kind, created_at) VALUES ('dav', 'caldav', 1);
             INSERT INTO pim_collections (id, source_id, kind, settings)
                 VALUES ('c1', 'dav', 'calendar', '{}');",
        )
        .unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
        // The rebuild kept every source and collection, and foreign keys
        // are on again.
        let fk: bool = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert!(fk);
        let collections: i64 = conn
            .query_row("SELECT COUNT(*) FROM pim_collections", [], |r| r.get(0))
            .unwrap();
        assert_eq!(collections, 1);
        let sources: Vec<String> = conn
            .prepare("SELECT id FROM pim_sources ORDER BY id")
            .unwrap()
            .query_map([], |r| r.get(0))
            .unwrap()
            .collect::<rusqlite::Result<_>>()
            .unwrap();
        assert_eq!(sources, ["dav", "local"]);
        // Exchange sources need their account (and only they have one).
        assert!(
            conn.execute(
                "INSERT INTO pim_sources (id, kind, created_at) VALUES ('exchange:x', 'exchange', 1)",
                []
            )
            .is_err()
        );
        conn.execute_batch(
            "INSERT INTO pim_sources (id, kind, account_id, created_at)
                 VALUES ('exchange:x', 'exchange', 'x', 1);
             INSERT INTO pim_collections (id, source_id, kind, settings)
                 VALUES ('xc', 'exchange:x', 'calendar', '{}');
             INSERT INTO exchange_pim_collections (collection_id, account_id, remote_id)
                 VALUES ('xc', 'x', 'AQMk');
             INSERT INTO exchange_folders (folder_id, account_id, remote_id)
                 VALUES ('fi', 'x', 'AAMk');
             INSERT INTO exchange_items (message_id, account_id, inference)
                 VALUES ('m', 'x', 'focused');",
        )
        .unwrap();
        assert!(
            conn.execute(
                "INSERT INTO pim_sources (id, kind, account_id, created_at)
                 VALUES ('dav2', 'caldav', 'x', 1)",
                []
            )
            .is_err()
        );
        assert!(
            conn.execute("UPDATE exchange_items SET inference = 'primary'", [])
                .is_err()
        );
        // Deleting the account removes its source, collections and rows.
        conn.execute("DELETE FROM accounts WHERE id = 'x'", [])
            .unwrap();
        let left: i64 = conn
            .query_row(
                "SELECT (SELECT COUNT(*) FROM pim_sources WHERE kind = 'exchange')
                     + (SELECT COUNT(*) FROM pim_collections WHERE id = 'xc')
                     + (SELECT COUNT(*) FROM exchange_pim_collections)
                     + (SELECT COUNT(*) FROM exchange_folders)
                     + (SELECT COUNT(*) FROM exchange_items)",
                [],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(left, 0);
    }

    #[test]
    fn rebuild_steps_refuse_broken_references() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        let steps = [
            Migration::sql(
                "CREATE TABLE p (id TEXT PRIMARY KEY);
                 CREATE TABLE c (p TEXT REFERENCES p(id) ON DELETE CASCADE);
                 INSERT INTO p VALUES ('a'); INSERT INTO c VALUES ('a');",
            ),
            // Drops the referenced row without the cascade (foreign keys
            // are off): the check refuses the step.
            Migration::rebuild(
                "CREATE TABLE p2 (id TEXT PRIMARY KEY); DROP TABLE p; ALTER TABLE p2 RENAME TO p;",
            ),
        ];
        assert!(migrate_with(&mut conn, &steps).is_err());
        assert_eq!(current_version(&conn).unwrap(), 1);
        let kept: i64 = conn
            .query_row("SELECT COUNT(*) FROM c", [], |r| r.get(0))
            .unwrap();
        assert_eq!(kept, 1, "nothing cascaded");
        let fk: bool = conn
            .query_row("PRAGMA foreign_keys", [], |r| r.get(0))
            .unwrap();
        assert!(fk, "foreign keys are restored after a failed step");
    }

    #[test]
    fn refuses_newer_schema() {
        let mut conn = Connection::open_in_memory().unwrap();
        migrate(&mut conn).unwrap();
        conn.execute(
            "INSERT INTO schema_version (version) VALUES (?1)",
            [SCHEMA_VERSION + 1],
        )
        .unwrap();
        match migrate(&mut conn) {
            Err(Error::SchemaTooNew { found, supported }) => {
                assert_eq!(found, SCHEMA_VERSION + 1);
                assert_eq!(supported, SCHEMA_VERSION);
            }
            other => panic!("expected SchemaTooNew, got {other:?}"),
        }
    }

    #[test]
    fn v15_keeps_keys_and_allows_smartcards_and_server_threads() {
        let mut conn = Connection::open_in_memory().unwrap();
        conn.pragma_update(None, "foreign_keys", true).unwrap();
        migrate_to_version(&mut conn, 14).unwrap();
        conn.execute_batch(
            "INSERT INTO crypto_keys (id, technology, fingerprint, algorithm, created_at, source,
                 public_data, secret_storage, secret_data, secret_ref, imported_at, updated_at)
                 VALUES (7, 'openpgp', 'AB', 'Ed25519', 1, 'generated', x'01', 'keychain',
                         x'02', 'keychain:nail/openpgp/AB', 1, 1);
             INSERT INTO crypto_key_emails (key_id, email) VALUES (7, 'ada@x');",
        )
        .unwrap();
        conn.execute_batch(GMAIL_MESSAGE).unwrap();
        assert_eq!(migrate(&mut conn).unwrap(), SCHEMA_VERSION);
        let (fpr, email): (String, String) = conn
            .query_row(
                "SELECT k.fingerprint, e.email FROM crypto_keys k
                 JOIN crypto_key_emails e ON e.key_id = k.id WHERE k.id = 7",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!((fpr.as_str(), email.as_str()), ("AB", "ada@x"));
        // Smartcard keys: no secret data, a reference.
        conn.execute(
            "INSERT INTO crypto_keys (technology, fingerprint, algorithm, created_at, source,
                 public_data, secret_storage, secret_ref, imported_at, updated_at)
             VALUES ('smime', 'CD', 'RSA 2048', 1, 'smartcard', x'01', 'smartcard',
                     '{}', 1, 1)",
            [],
        )
        .unwrap();
        assert!(
            conn.execute("UPDATE crypto_keys SET source = 'bogus' WHERE id = 7", [])
                .is_err()
        );
        // The email rows still cascade with their key.
        conn.execute("DELETE FROM crypto_keys WHERE id = 7", [])
            .unwrap();
        let emails: i64 = conn
            .query_row("SELECT COUNT(*) FROM crypto_key_emails", [], |r| r.get(0))
            .unwrap();
        assert_eq!(emails, 0);
        conn.execute(
            "UPDATE messages SET remote_thread_id = 't1' WHERE id = 'm'",
            [],
        )
        .unwrap();
    }

    /// Released migrations are append-only: their SQL (the files in
    /// `src/migrations/` and the v18 triggers built by macros) stays byte
    /// for byte what shipped.
    #[test]
    fn released_migrations_are_unchanged() {
        use sha2::Digest;
        let released = [
            "e6a6d76fde728f69397905b223cc68848f45cac336149df03e2b3ea64178df9f",
            "c35316951c134dacfd40bc0900e42ebcec222abcaa323b7b4ba04b623987e6d2",
            "e23956510d2a52058b2447d9da4cb5e1611a7d6ffb19323a84a9fd14024216b4",
            "ac2536f7825325b47fafe81c3919cda1f36e9aa926cc6cc21f0a5daf79591c49",
            "f761be55ced7c21ba7146c2cf0b692b600474eb858f77880928ab8be9df01cbb",
            "f3760de16d67473bab9d8492c74020d91416b032e5e27c1d0af28dbb09d24a79",
            "cc4af4bd907bb7243c188607d89846f4c537e32311995da34c1f0e1ffdb591cf",
            "ad2a6d9396e7c7c2278b446337d8fdcdfc2c20eccb049434e85888d7d642f3c5",
            "911270c297b706a0e3e730049df9fa9fb4044e75d511d374a91d32773e24bba8",
            "fb6c8d1aa231036248f80600eaf6e4a8cb46d9a025c6aa4ad676c84a71ff6785",
            "6a03f6e4b89719c370973eceea05c4a247492074c30e241df4169a592fcb9661",
            "db483759084c27f32db335cc5cbaaae5fc643486d8eab0786959af88d3a33103",
            "ad8e8c62cd552bfc2d30fa5c927e053da8ad9789f6d286a8423b50db9093ce06",
            "b9e4f17d5f5edc255819d2a4e3e05bbb61994da19c062f94ae7e15546e49f4e4",
            "bf4c5ee11e64ebdc5174091d0805dc776b756b76eaa673807190cad854b4b8f5",
            "a9de603450510614947961a91cdc9a0b18b657416e832908cc395c8d9c281bbc",
            "9b55836401c702f283ebf299bcf49eccb329c94ff3c37c57da939ba4906101ef",
            "91907f9094c4674f5d4a72eb0bbb3df765d320b4c50d0ba49afbf137a0953fc7",
        ];
        assert!(MIGRATIONS.len() >= released.len());
        for (n, (m, want)) in MIGRATIONS.iter().zip(released).enumerate() {
            let got = hex::encode(sha2::Sha256::digest(m.sql.as_bytes()));
            assert_eq!(got, want, "migration to v{} changed", n + 1);
        }
    }
}
