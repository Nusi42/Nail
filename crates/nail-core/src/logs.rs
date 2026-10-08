//! The recent log lines of this process, for the diagnostics bundle (M4c,
//! ADR 0099). Nail writes no log files; the binaries tee their `tracing`
//! output through [`TeeWriter`] (stderr + an in-memory ring of the last
//! [`MAX_LINES`] lines, ANSI colours removed). The bundle's `logs` item reads
//! [`recent`] and redacts it like every other text.

use std::collections::VecDeque;
use std::io::{self, Write};
use std::sync::{Mutex, OnceLock, PoisonError};

/// Lines kept.
pub const MAX_LINES: usize = 2000;

/// Longest line kept (bytes; longer lines are cut).
pub const MAX_LINE_BYTES: usize = 4096;

struct Ring {
    lines: VecDeque<String>,
    partial: String,
}

fn ring() -> &'static Mutex<Ring> {
    static RING: OnceLock<Mutex<Ring>> = OnceLock::new();
    RING.get_or_init(|| {
        Mutex::new(Ring {
            lines: VecDeque::new(),
            partial: String::new(),
        })
    })
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
        let end = line.floor_char_boundary(MAX_LINE_BYTES);
        line.truncate(end);
    }
    line
}

/// Records log output (what [`TeeWriter`] writes besides stderr).
pub fn record(bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    let mut ring = ring().lock().unwrap_or_else(PoisonError::into_inner);
    ring.partial.push_str(&text);
    while let Some(end) = ring.partial.find('\n') {
        let line: String = ring.partial.drain(..=end).collect();
        let line = cut(strip_ansi(line.trim_end_matches(['\r', '\n'])));
        ring.lines.push_back(line);
        if ring.lines.len() > MAX_LINES {
            ring.lines.pop_front();
        }
    }
    if ring.partial.len() > MAX_LINE_BYTES {
        let partial = std::mem::take(&mut ring.partial);
        ring.lines.push_back(cut(strip_ansi(&partial)));
    }
}

/// The recorded lines, oldest first.
pub fn recent() -> Vec<String> {
    ring()
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .lines
        .iter()
        .cloned()
        .collect()
}

/// A `tracing_subscriber` writer: `fmt().with_writer(|| TeeWriter)` writes
/// to stderr and records the lines.
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
    fn lines_are_split_stripped_and_kept() {
        record(b"\x1b[32m INFO\x1b[0m first line\nsecond ");
        record(b"line\r\n");
        let lines = recent();
        let n = lines.len();
        assert!(n >= 2);
        assert_eq!(lines[n - 2], " INFO first line");
        assert_eq!(lines[n - 1], "second line");
        assert_eq!(strip_ansi("\x1b[1;31mred\x1b[0m"), "red");
        assert_eq!(cut("é".repeat(MAX_LINE_BYTES)).len(), MAX_LINE_BYTES);
    }
}
