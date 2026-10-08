//! The core's connection logs: one bounded in-memory ring per
//! [`ConnectionLogScope`], optionally mirrored to rotating files
//! ([`ConnectionLogs::set_disk`]), with change notices for the UI.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError, Weak};

use nail_api::connlog::{
    CONNECTION_LOG_CAPACITY, CONNECTION_LOG_FILE_BYTES, ConnectionLog, ConnectionLogEntry,
    ConnectionLogScope,
};

use crate::files::{RotatingFile, remove_generations};
use crate::tap::{WireRecord, WireSink, WireTap};

#[derive(Debug, Default)]
struct Ring {
    entries: VecDeque<ConnectionLogEntry>,
    last_seq: u64,
    dropped: u64,
}

#[derive(Default)]
struct Inner {
    rings: BTreeMap<ConnectionLogScope, Ring>,
    taps: BTreeMap<ConnectionLogScope, WireTap>,
    disk: Option<PathBuf>,
    files: BTreeMap<ConnectionLogScope, RotatingFile>,
    changed: BTreeSet<ConnectionLogScope>,
}

/// Every scope's connection log.
pub struct ConnectionLogs {
    inner: Mutex<Inner>,
    capacity: usize,
    me: Weak<ConnectionLogs>,
}

impl std::fmt::Debug for ConnectionLogs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ConnectionLogs")
            .field("capacity", &self.capacity)
            .finish_non_exhaustive()
    }
}

struct ScopeSink {
    logs: Weak<ConnectionLogs>,
    scope: ConnectionLogScope,
}

impl WireSink for ScopeSink {
    fn record(&self, record: WireRecord) {
        if let Some(logs) = self.logs.upgrade() {
            logs.record(&self.scope, record);
        }
    }
}

/// The file name an exported log is saved under
/// (`nail-connection-log-account-work.txt`).
pub fn export_name(scope: &ConnectionLogScope) -> String {
    format!(
        "nail-connection-log-{}.txt",
        file_name(scope).trim_end_matches(".log")
    )
}

/// The file name of a scope's log on disk (`account-work.log`): only
/// letters, digits, `-` and `_` from the id, so no id can name a path.
pub fn file_name(scope: &ConnectionLogScope) -> String {
    let clean = |id: &str| -> String {
        id.chars()
            .take(64)
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                    c
                } else {
                    '_'
                }
            })
            .collect()
    };
    match scope {
        ConnectionLogScope::Account { account_id } => {
            format!("account-{}.log", clean(account_id.as_str()))
        }
        ConnectionLogScope::PimSource { source_id } => {
            format!("source-{}.log", clean(source_id.as_str()))
        }
        ConnectionLogScope::Setup => "setup.log".to_owned(),
    }
}

fn lock<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

fn now() -> String {
    chrono::Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
}

impl ConnectionLogs {
    /// Empty logs keeping [`CONNECTION_LOG_CAPACITY`] entries per scope.
    pub fn new() -> Arc<Self> {
        Self::with_capacity(CONNECTION_LOG_CAPACITY)
    }

    /// Empty logs keeping `capacity` entries per scope (tests).
    pub fn with_capacity(capacity: usize) -> Arc<Self> {
        Arc::new_cyclic(|me| Self {
            inner: Mutex::new(Inner::default()),
            capacity: capacity.max(1),
            me: me.clone(),
        })
    }

    /// The tap of `scope` (one per scope, so the secrets it learned stay).
    pub fn tap(&self, scope: &ConnectionLogScope) -> WireTap {
        let mut inner = lock(&self.inner);
        inner
            .taps
            .entry(scope.clone())
            .or_insert_with(|| {
                WireTap::new(Arc::new(ScopeSink {
                    logs: self.me.clone(),
                    scope: scope.clone(),
                }))
            })
            .clone()
    }

    /// Stores one record (and appends it to the scope's file when kept on
    /// disk).
    pub fn record(&self, scope: &ConnectionLogScope, record: WireRecord) {
        let mut inner = lock(&self.inner);
        let ring = inner.rings.entry(scope.clone()).or_default();
        ring.last_seq += 1;
        let entry = ConnectionLogEntry {
            seq: ring.last_seq,
            at: now(),
            protocol: record.protocol,
            connection: record.connection,
            direction: record.direction,
            level: record.level,
            text: record.text,
        };
        let line = entry.line();
        ring.entries.push_back(entry);
        while ring.entries.len() > self.capacity {
            ring.entries.pop_front();
            ring.dropped += 1;
        }
        inner.changed.insert(scope.clone());
        if let Some(dir) = inner.disk.clone() {
            let file = inner.files.entry(scope.clone()).or_insert_with(|| {
                RotatingFile::new(dir.join(file_name(scope)), CONNECTION_LOG_FILE_BYTES)
            });
            if let Err(err) = file.append_line(&line) {
                // Not logged through tracing: a failing disk must not feed
                // back into the log; the in-memory ring still has the entry.
                drop(err);
            }
        }
    }

    /// The scope's entries after `after` (all kept when `None`), at most
    /// the newest `limit`.
    pub fn get(
        &self,
        scope: &ConnectionLogScope,
        after: Option<u64>,
        limit: Option<usize>,
    ) -> ConnectionLog {
        let inner = lock(&self.inner);
        let ring = inner.rings.get(scope);
        let mut entries: Vec<ConnectionLogEntry> = ring
            .map(|r| {
                r.entries
                    .iter()
                    .filter(|e| after.is_none_or(|a| e.seq > a))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        if let Some(limit) = limit
            && entries.len() > limit
        {
            entries.drain(..entries.len() - limit);
        }
        ConnectionLog {
            scope: scope.clone(),
            entries,
            last_seq: ring.map_or(0, |r| r.last_seq),
            dropped: ring.map_or(0, |r| r.dropped),
            capacity: u32::try_from(self.capacity).unwrap_or(u32::MAX),
            keep_on_disk: inner.disk.is_some(),
        }
    }

    /// The newest position of `scope` (0 when nothing was recorded).
    pub fn last_seq(&self, scope: &ConnectionLogScope) -> u64 {
        lock(&self.inner).rings.get(scope).map_or(0, |r| r.last_seq)
    }

    /// The scope's entries as text, one line each (copy / export).
    pub fn text(&self, scope: &ConnectionLogScope) -> String {
        let inner = lock(&self.inner);
        let mut out = String::new();
        if let Some(ring) = inner.rings.get(scope) {
            for entry in &ring.entries {
                out.push_str(&entry.line());
                out.push('\n');
            }
        }
        out
    }

    /// Every scope with entries in memory.
    pub fn scopes(&self) -> Vec<ConnectionLogScope> {
        lock(&self.inner)
            .rings
            .iter()
            .filter(|(_, ring)| !ring.entries.is_empty())
            .map(|(scope, _)| scope.clone())
            .collect()
    }

    /// Forgets the scope's entries (positions continue) and deletes its file.
    pub fn clear(&self, scope: &ConnectionLogScope) {
        let mut inner = lock(&self.inner);
        if let Some(ring) = inner.rings.get_mut(scope) {
            ring.dropped += u64::try_from(ring.entries.len()).unwrap_or(u64::MAX);
            ring.entries.clear();
        }
        inner.changed.insert(scope.clone());
        let file = inner.files.remove(scope);
        let path = inner.disk.as_ref().map(|dir| dir.join(file_name(scope)));
        drop(inner);
        match (file, path) {
            (Some(file), _) => drop(file.remove()),
            (None, Some(path)) => drop(remove_generations(&path)),
            (None, None) => {}
        }
    }

    /// Mirrors new entries to files in `dir` (`Some`), or stops and deletes
    /// the folder (`None`; `delete` names the folder to remove).
    pub fn set_disk(&self, dir: Option<PathBuf>, delete: Option<&Path>) -> std::io::Result<()> {
        {
            let mut inner = lock(&self.inner);
            inner.files.clear();
            inner.disk = dir;
        }
        if let Some(folder) = delete {
            match std::fs::remove_dir_all(folder) {
                Err(err) if err.kind() != std::io::ErrorKind::NotFound => return Err(err),
                _ => {}
            }
        }
        Ok(())
    }

    /// Whether entries are mirrored to disk.
    pub fn on_disk(&self) -> bool {
        lock(&self.inner).disk.is_some()
    }

    /// The scopes that changed since the last call, with their newest
    /// position (for throttled change notices).
    pub fn take_changed(&self) -> Vec<(ConnectionLogScope, u64)> {
        let mut inner = lock(&self.inner);
        let changed = std::mem::take(&mut inner.changed);
        changed
            .into_iter()
            .map(|scope| {
                let seq = inner.rings.get(&scope).map_or(0, |r| r.last_seq);
                (scope, seq)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use nail_api::connlog::{WireDirection, WireLevel, WireProtocol};

    use super::*;
    use crate::Part;

    fn scope() -> ConnectionLogScope {
        ConnectionLogScope::account(&"work".into())
    }

    #[test]
    fn rings_are_bounded_and_paged() {
        let logs = ConnectionLogs::with_capacity(3);
        let tap = logs.tap(&scope());
        let conn = tap.connect(WireProtocol::Imap, "imap.example.org", 993);
        for n in 0..4 {
            conn.sent(&[Part::Text(&format!("A000{n} NOOP"))]);
        }
        let log = logs.get(&scope(), None, None);
        assert_eq!(log.entries.len(), 3);
        assert_eq!(log.last_seq, 5);
        assert_eq!(log.dropped, 2);
        assert_eq!(log.entries[0].text, "A0001 NOOP");
        assert_eq!(logs.get(&scope(), Some(4), None).entries.len(), 1);
        assert_eq!(logs.get(&scope(), None, Some(1)).entries[0].seq, 5);
        assert_eq!(logs.take_changed(), vec![(scope(), 5)]);
        assert_eq!(logs.take_changed(), []);
        assert!(logs.text(&scope()).contains(" imap #"));
        logs.clear(&scope());
        assert_eq!(logs.get(&scope(), None, None).entries, []);
        assert_eq!(logs.last_seq(&scope()), 5, "positions continue");
        let other = ConnectionLogScope::Setup;
        assert_eq!(logs.get(&other, None, None).last_seq, 0);
        drop(conn);
    }

    #[test]
    fn disk_mirror_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let folder = dir.path().join("connections");
        let logs = ConnectionLogs::new();
        logs.set_disk(Some(folder.clone()), None).unwrap();
        logs.record(
            &scope(),
            WireRecord {
                protocol: WireProtocol::Smtp,
                connection: 1,
                direction: WireDirection::Sent,
                level: WireLevel::Info,
                text: "EHLO x".into(),
            },
        );
        let file = folder.join("account-work.log");
        assert!(
            std::fs::read_to_string(&file)
                .unwrap()
                .contains("smtp #1 C: EHLO x")
        );
        assert!(logs.get(&scope(), None, None).keep_on_disk);
        logs.clear(&scope());
        assert!(!file.exists());
        logs.set_disk(None, Some(&folder)).unwrap();
        assert!(!folder.exists());
        assert!(!logs.on_disk());
    }

    #[test]
    fn file_names_are_safe() {
        assert_eq!(
            file_name(&ConnectionLogScope::account(&"../evil\\x".into())),
            "account-___evil_x.log"
        );
        assert_eq!(
            file_name(&ConnectionLogScope::PimSource {
                source_id: "work-calendars".into()
            }),
            "source-work-calendars.log"
        );
        assert_eq!(file_name(&ConnectionLogScope::Setup), "setup.log");
        assert_eq!(
            export_name(&ConnectionLogScope::account(&"work".into())),
            "nail-connection-log-account-work.txt"
        );
    }
}
