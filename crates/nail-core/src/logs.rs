//! The application log (ADR 0190; before: M4c, ADR 0099). The binaries tee
//! their `tracing` output through [`TeeWriter`]: stderr, an in-memory ring
//! of the last [`MAX_LINES`] lines and — once a data directory is known
//! ([`attach_file`]) — `<data-dir>/logs/app.log`, rotated at
//! [`APP_LOG_FILE_BYTES`] with two older files.
//!
//! Every line passes the one redactor ([`redact_line`]: secrets, subjects /
//! display names, masked addresses) before it reaches the ring or the file,
//! so neither holds a password, a token or mail content. The diagnostics
//! bundle's `logs` item and `GetAppLog` read the ring.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock, PoisonError};

use nail_api::connlog::{APP_LOG_FILE, APP_LOG_FILE_BYTES, LOG_DIR};
use nail_connlog::RotatingFile;
use nail_connlog::redact::{mask_addresses, redact_secrets, redact_subjects};

/// Lines kept.
pub const MAX_LINES: usize = 2000;

/// Longest line kept (bytes; longer lines are cut).
pub const MAX_LINE_BYTES: usize = 4096;

struct Ring {
    lines: VecDeque<String>,
    partial: String,
    file: Option<RotatingFile>,
}

fn ring() -> &'static Mutex<Ring> {
    static RING: OnceLock<Mutex<Ring>> = OnceLock::new();
    RING.get_or_init(|| {
        Mutex::new(Ring {
            lines: VecDeque::new(),
            partial: String::new(),
            file: None,
        })
    })
}

fn lock() -> std::sync::MutexGuard<'static, Ring> {
    ring().lock().unwrap_or_else(PoisonError::into_inner)
}

/// Removes ANSI escape sequences (`ESC [ … letter`).
fn strip_ansi(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\u{1b}' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for d in chars.by_ref() {
                    if d.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

fn cut(mut line: String) -> String {
    if line.len() > MAX_LINE_BYTES {
        let mut end = MAX_LINE_BYTES;
        while !line.is_char_boundary(end) {
            end -= 1;
        }
        line.truncate(end);
    }
    line
}

/// One log line as the ring and the file keep it: secrets, subjects and
/// display names removed, addresses masked.
pub fn redact_line(line: &str) -> String {
    mask_addresses(&redact_subjects(&redact_secrets(line)))
}

impl Ring {
    fn push(&mut self, raw: &str) {
        let line = cut(redact_line(&strip_ansi(raw)));
        if let Some(file) = &mut self.file
            && let Err(err) = file.append_line(&line)
        {
            // A failing disk must not loop back into the log; stop writing.
            drop(err);
            self.file = None;
        }
        self.lines.push_back(line);
        if self.lines.len() > MAX_LINES {
            self.lines.pop_front();
        }
    }
}

/// Records log output (what [`TeeWriter`] writes besides stderr).
pub fn record(bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    let mut ring = lock();
    ring.partial.push_str(&text);
    while let Some(end) = ring.partial.find('\n') {
        let line: String = ring.partial.drain(..=end).collect();
        ring.push(line.trim_end_matches(['\r', '\n']));
    }
    if ring.partial.len() > MAX_LINE_BYTES {
        let partial = std::mem::take(&mut ring.partial);
        ring.push(&partial);
    }
}

/// The recorded lines, oldest first.
pub fn recent() -> Vec<String> {
    lock().lines.iter().cloned().collect()
}

/// The newest `n` recorded lines, oldest first.
pub fn recent_lines(n: usize) -> Vec<String> {
    let ring = lock();
    let skip = ring.lines.len().saturating_sub(n);
    ring.lines.iter().skip(skip).cloned().collect()
}

/// Starts writing `<data_dir>/logs/app.log` (the lines recorded so far
/// first). Returns the file's path.
pub fn attach_file(data_dir: &Path) -> io::Result<PathBuf> {
    let path = data_dir.join(LOG_DIR).join(APP_LOG_FILE);
    let mut file = RotatingFile::new(path.clone(), APP_LOG_FILE_BYTES);
    let mut ring = lock();
    for line in &ring.lines {
        file.append_line(line)?;
    }
    ring.file = Some(file);
    Ok(path)
}

/// Where `app.log` is written, if anywhere.
pub fn file_path() -> Option<PathBuf> {
    lock().file.as_ref().map(|f| f.path().to_owned())
}

/// A `tracing_subscriber` writer: `fmt().with_writer(|| TeeWriter)` writes
/// to stderr and records the (redacted) lines.
#[derive(Debug, Clone, Copy, Default)]
pub struct TeeWriter;

impl Write for TeeWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        record(buf);
        io::stderr().write_all(buf)?;
        Ok(buf.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        io::stderr().flush()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_split_stripped_redacted_and_kept() {
        record(b"\x1b[32m INFO\x1b[0m first line\nsecond ");
        record(b"line\r\n");
        record(b"WARN sync failed for sam@example.net password=hunter2 subject=\"Salary\"\n");
        let lines = recent();
        let n = lines.len();
        assert!(n >= 3);
        assert_eq!(lines[n - 3], " INFO first line");
        assert_eq!(lines[n - 2], "second line");
        let last = &lines[n - 1];
        assert!(last.contains("s***@example.net"), "{last}");
        assert!(
            !last.contains("hunter2") && !last.contains("Salary"),
            "{last}"
        );
        assert_eq!(recent_lines(1), std::slice::from_ref(last));
        assert_eq!(strip_ansi("\x1b[1;31mred\x1b[0m"), "red");
        assert_eq!(cut("é".repeat(MAX_LINE_BYTES)).len(), MAX_LINE_BYTES);

        // The file gets the lines recorded before and after attaching (one
        // test: the ring is process-wide).
        let dir = tempfile::tempdir().unwrap();
        record(b"before attaching\n");
        let path = attach_file(dir.path()).unwrap();
        assert_eq!(file_path().as_deref(), Some(path.as_path()));
        record(b"after attaching token=abc123\n");
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.contains("before attaching"));
        assert!(text.contains("after attaching token=<redacted>"), "{text}");
        // Other tests share the process-wide ring: detach again.
        lock().file = None;
    }
}
