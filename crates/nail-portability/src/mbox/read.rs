//! Streaming mbox reader: one message at a time, never the whole file.
//!
//! * Messages start at `From ` lines that follow an empty line (or the
//!   start of the file); a `From ` line that does not follow an empty line
//!   still starts a message when it looks like an envelope line (sender,
//!   then an `asctime` date).
//! * **mboxrd / mboxo**: lines matching `>+From ` lose one `>` (mboxrd; the
//!   same as mboxo for `>From `, the only quoting mboxo writes). With
//!   [`MboxDialect::Mboxo`] only `>From ` is unquoted.
//! * **mboxcl / mboxcl2**: a `Content-Length` header is trusted when the
//!   message then ends at an empty line + `From ` line (or the end of the
//!   file); `From ` lines inside such a body are content. Bodies with
//!   unquoted `From ` lines are mboxcl2 (no unquoting), else mboxcl.
//! * Lines longer than [`MAX_LINE_CHUNK`] are read in pieces, messages
//!   larger than the limit are skipped while reading (only their size is
//!   kept), so memory stays bounded by the largest accepted message.

use std::collections::VecDeque;
use std::io::{self, BufRead};

use chrono::NaiveDateTime;

/// Longest piece of a line read at once.
pub const MAX_LINE_CHUNK: usize = 1024 * 1024;

/// mbox dialects.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MboxDialect {
    /// `>+From ` quoting, reversible.
    Mboxrd,
    /// `>From ` quoting only.
    Mboxo,
    /// `Content-Length` with mboxo quoting.
    Mboxcl,
    /// `Content-Length`, no quoting.
    Mboxcl2,
}

/// Options for reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MboxReadOptions {
    /// Messages above this are skipped ([`MboxEntry::too_large`]).
    pub max_message_bytes: u64,
    /// Keep the message bytes (`false` = only count, for previews).
    pub keep_bytes: bool,
    /// Force a dialect; `None` = detect per message.
    pub dialect: Option<MboxDialect>,
}

impl Default for MboxReadOptions {
    fn default() -> Self {
        Self {
            max_message_bytes: nail_api::portability::MAX_IMPORT_MESSAGE_BYTES,
            keep_bytes: true,
            dialect: None,
        }
    }
}

/// One message of an mbox file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MboxEntry {
    /// 1-based position in the file.
    pub index: u64,
    /// Byte offset of its `From ` line.
    pub offset: u64,
    /// The `From ` line without `From ` and the line end.
    pub envelope: String,
    /// The message, unquoted (empty when too large or not kept).
    pub bytes: Vec<u8>,
    /// Bytes the message takes in the file (with quoting, without the
    /// `From ` line and the separating empty line).
    pub size_on_disk: u64,
    /// Whether it was larger than the limit (then `bytes` is empty).
    pub too_large: bool,
    /// Whether it is cut off (the file ends inside its headers, or before
    /// its `Content-Length`).
    pub truncated: bool,
    /// Whether its headers never ended (nothing but headers, cut off).
    pub headers_unfinished: bool,
    /// The dialect this message showed, if it showed one.
    pub dialect: Option<MboxDialect>,
}

impl MboxEntry {
    /// When the message was delivered, from the envelope's date.
    pub fn received_at(&self) -> Option<i64> {
        envelope_date(&self.envelope)
    }

    /// The envelope sender.
    pub fn envelope_sender(&self) -> Option<&str> {
        self.envelope.split_whitespace().next()
    }
}

#[derive(Debug, Clone)]
struct Line {
    bytes: Vec<u8>,
    offset: u64,
    /// Whether the piece starts a line (else it continues a long one).
    starts_line: bool,
}

impl Line {
    fn ends_line(&self) -> bool {
        self.bytes.last() == Some(&b'\n')
    }

    fn is_blank(&self) -> bool {
        self.starts_line && (self.bytes == b"\n" || self.bytes == b"\r\n")
    }

    fn is_from(&self) -> bool {
        self.starts_line && self.bytes.starts_with(b"From ")
    }
}

/// Streams the messages of an mbox file.
pub struct MboxReader<R> {
    reader: R,
    options: MboxReadOptions,
    offset: u64,
    at_line_start: bool,
    replay: VecDeque<Line>,
    pending_from: Option<Line>,
    index: u64,
    started: bool,
    done: bool,
}

impl<R: BufRead> MboxReader<R> {
    /// A reader over `reader`.
    pub fn new(reader: R, options: MboxReadOptions) -> Self {
        Self {
            reader,
            options,
            offset: 0,
            at_line_start: true,
            replay: VecDeque::new(),
            pending_from: None,
            index: 0,
            started: false,
            done: false,
        }
    }

    /// Bytes read from the underlying reader so far.
    pub fn bytes_read(&self) -> u64 {
        self.offset
    }

    fn read_piece(&mut self) -> io::Result<Option<Line>> {
        if let Some(line) = self.replay.pop_front() {
            return Ok(Some(line));
        }
        let offset = self.offset;
        let starts_line = self.at_line_start;
        let mut bytes = Vec::new();
        loop {
            let buf = match self.reader.fill_buf() {
                Ok(buf) => buf,
                Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                Err(e) => return Err(e),
            };
            if buf.is_empty() {
                break;
            }
            let room = MAX_LINE_CHUNK - bytes.len();
            let (take, newline) = match buf.iter().take(room).position(|&b| b == b'\n') {
                Some(i) => (i + 1, true),
                None => (buf.len().min(room), false),
            };
            bytes.extend_from_slice(&buf[..take]);
            self.reader.consume(take);
            if newline || bytes.len() >= MAX_LINE_CHUNK {
                break;
            }
        }
        if bytes.is_empty() {
            return Ok(None);
        }
        self.offset += bytes.len() as u64;
        self.at_line_start = bytes.last() == Some(&b'\n');
        Ok(Some(Line {
            bytes,
            offset,
            starts_line,
        }))
    }

    /// The next message, `None` at the end. The first call fails with
    /// `InvalidData` when the file does not start with a `From ` line.
    pub fn next_entry(&mut self) -> io::Result<Option<MboxEntry>> {
        if self.done {
            return Ok(None);
        }
        if !self.started {
            self.started = true;
            loop {
                match self.read_piece()? {
                    None => {
                        self.done = true;
                        return Ok(None);
                    }
                    Some(line) if line.is_blank() => {}
                    Some(line) if line.is_from() => {
                        self.pending_from = Some(line);
                        break;
                    }
                    Some(_) => {
                        self.done = true;
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "the file does not start with a \"From \" line",
                        ));
                    }
                }
            }
        }
        let Some(from) = self.pending_from.take() else {
            self.done = true;
            return Ok(None);
        };
        // A `From ` line longer than a chunk: drop the rest of it.
        let mut from_line = from;
        from_line.bytes.truncate(4096);
        let mut ended = from_line.ends_line();
        while !ended {
            match self.read_piece()? {
                Some(more) => ended = more.ends_line(),
                None => break,
            }
        }
        self.index += 1;
        let envelope = String::from_utf8_lossy(&from_line.bytes[5..])
            .trim_end_matches(['\r', '\n'])
            .trim()
            .to_owned();
        let mut message = MessageBuilder::new(self.options);
        let mut entry = MboxEntry {
            index: self.index,
            offset: from_line.offset,
            envelope,
            bytes: Vec::new(),
            size_on_disk: 0,
            too_large: false,
            truncated: false,
            headers_unfinished: false,
            dialect: None,
        };

        // Headers.
        let mut content_length: Option<u64> = None;
        loop {
            let Some(line) = self.read_piece()? else {
                entry.headers_unfinished = !message.is_empty();
                entry.truncated = entry.headers_unfinished;
                self.done = true;
                return Ok(Some(message.finish(entry, false)));
            };
            if line.is_from() && looks_like_envelope(&line.bytes) {
                // A message with headers only, no empty line.
                self.pending_from = Some(line);
                return Ok(Some(message.finish(entry, false)));
            }
            if line.starts_line
                && let Some(value) = header_line_value(&line.bytes, b"content-length")
            {
                content_length = value.trim().parse().ok();
            }
            let blank = line.is_blank();
            message.push_header(&line);
            if blank {
                break;
            }
        }
        // The headers ended with an empty line.
        let mut prev_blank = true;

        // Body with a trusted Content-Length?
        if let Some(length) = content_length
            && length <= self.options.max_message_bytes
            && self.options.dialect != Some(MboxDialect::Mboxrd)
            && self.options.dialect != Some(MboxDialect::Mboxo)
        {
            let mut body: Vec<Line> = Vec::new();
            let mut read = 0u64;
            while read < length {
                match self.read_piece()? {
                    Some(line) => {
                        read += line.bytes.len() as u64;
                        body.push(line);
                    }
                    None => break,
                }
            }
            let exact = read == length;
            let next = if exact { self.read_piece()? } else { None };
            let fits = exact
                && match &next {
                    None => true,
                    Some(line) if line.is_from() => true,
                    Some(line) if line.is_blank() => {
                        let after = self.read_piece()?;
                        let ok = after.as_ref().is_none_or(Line::is_from);
                        if let Some(after) = after {
                            self.replay.push_front(after);
                        }
                        ok
                    }
                    Some(_) => false,
                };
            // Cut off before its length: trusted unless another message
            // starts inside (then the length is wrong).
            let cut_off = read < length
                && !body.iter().enumerate().any(|(i, line)| {
                    line.is_from()
                        && (i == 0 || body[i - 1].is_blank())
                        && looks_like_envelope(&line.bytes)
                });
            if fits || cut_off {
                // Trusted (or the file ends early: truncated, kept).
                if read < length {
                    entry.truncated = true;
                }
                let unquoted_from = body.iter().any(Line::is_from);
                let dialect = match self.options.dialect {
                    Some(d) => d,
                    None if unquoted_from => MboxDialect::Mboxcl2,
                    None => MboxDialect::Mboxcl,
                };
                entry.dialect = Some(dialect);
                for line in &body {
                    message.push_body(line, dialect);
                }
                // The separator: an empty line, then a `From ` line.
                match next {
                    Some(line) if line.is_from() => self.pending_from = Some(line),
                    Some(_blank) => {
                        if let Some(line) = self.replay.pop_front() {
                            if line.is_from() {
                                self.pending_from = Some(line);
                            } else {
                                self.replay.push_front(line);
                            }
                        }
                    }
                    None => self.done = self.replay.is_empty(),
                }
                return Ok(Some(message.finish(entry, false)));
            }
            // Content-Length is wrong: read the lines again as usual.
            if let Some(next) = next {
                self.replay.push_front(next);
            }
            for line in body.into_iter().rev() {
                self.replay.push_front(line);
            }
        }

        // Body up to the next `From ` line.
        let dialect = self.options.dialect.unwrap_or(MboxDialect::Mboxrd);
        loop {
            let Some(line) = self.read_piece()? else {
                self.done = true;
                return Ok(Some(message.finish(entry, prev_blank)));
            };
            if line.is_from() && (prev_blank || looks_like_envelope(&line.bytes)) {
                self.pending_from = Some(line);
                return Ok(Some(message.finish(entry, prev_blank)));
            }
            prev_blank = line.is_blank();
            if let Some(seen) = message.push_body(&line, dialect) {
                entry.dialect = Some(seen);
            }
        }
    }
}

impl<R: BufRead> Iterator for MboxReader<R> {
    type Item = io::Result<MboxEntry>;

    fn next(&mut self) -> Option<Self::Item> {
        self.next_entry().transpose()
    }
}

/// Collects one message's bytes within the size limit.
struct MessageBuilder {
    options: MboxReadOptions,
    bytes: Vec<u8>,
    size: u64,
    too_large: bool,
    /// Length of the last line pushed (to drop the separating empty line).
    last_line_len: usize,
    last_blank: bool,
}

impl MessageBuilder {
    fn new(options: MboxReadOptions) -> Self {
        Self {
            options,
            bytes: Vec::new(),
            size: 0,
            too_large: false,
            last_line_len: 0,
            last_blank: false,
        }
    }

    fn is_empty(&self) -> bool {
        self.size == 0
    }

    fn push_raw(&mut self, bytes: &[u8], disk_len: usize, blank: bool) {
        self.size += disk_len as u64;
        self.last_line_len = bytes.len();
        self.last_blank = blank;
        if self.too_large {
            return;
        }
        if self.size > self.options.max_message_bytes {
            self.too_large = true;
            self.bytes = Vec::new();
            return;
        }
        if self.options.keep_bytes {
            self.bytes.extend_from_slice(bytes);
        }
    }

    fn push_header(&mut self, line: &Line) {
        self.push_raw(&line.bytes, line.bytes.len(), line.is_blank());
    }

    /// Pushes a body line, unquoting `From ` lines; returns the dialect the
    /// quoting showed (`>>From ` = mboxrd, `>From ` = mboxo or mboxrd).
    fn push_body(&mut self, line: &Line, dialect: MboxDialect) -> Option<MboxDialect> {
        let blank = line.is_blank();
        if !line.starts_line || !line.bytes.starts_with(b">") {
            self.push_raw(&line.bytes, line.bytes.len(), blank);
            return None;
        }
        let quotes = line.bytes.iter().take_while(|&&b| b == b'>').count();
        if !line.bytes[quotes..].starts_with(b"From ") {
            self.push_raw(&line.bytes, line.bytes.len(), blank);
            return None;
        }
        let unquote = match dialect {
            MboxDialect::Mboxrd => true,
            MboxDialect::Mboxo | MboxDialect::Mboxcl => quotes == 1,
            MboxDialect::Mboxcl2 => false,
        };
        let seen = if quotes > 1 {
            Some(MboxDialect::Mboxrd)
        } else {
            Some(MboxDialect::Mboxo)
        };
        if unquote {
            self.push_raw(&line.bytes[1..], line.bytes.len(), blank);
        } else {
            self.push_raw(&line.bytes, line.bytes.len(), blank);
        }
        seen
    }

    /// Finishes the entry; `drop_separator` removes the final empty line
    /// (the one before the next `From ` line, or at the end of the file).
    fn finish(mut self, mut entry: MboxEntry, drop_separator: bool) -> MboxEntry {
        if drop_separator && self.last_blank && self.size > 0 {
            self.size -= self.last_line_len as u64;
            if !self.too_large && self.options.keep_bytes {
                let len = self.bytes.len().saturating_sub(self.last_line_len);
                self.bytes.truncate(len);
            }
        }
        entry.size_on_disk = self.size;
        entry.too_large = self.too_large;
        entry.bytes = if self.too_large {
            Vec::new()
        } else {
            self.bytes
        };
        entry
    }
}

/// The value of a header line if its name is `name` (lowercase).
fn header_line_value<'a>(line: &'a [u8], name: &[u8]) -> Option<&'a str> {
    let colon = line.iter().position(|&b| b == b':')?;
    if !line[..colon].eq_ignore_ascii_case(name) {
        return None;
    }
    std::str::from_utf8(&line[colon + 1..]).ok()
}

/// Whether a `From ` line looks like an envelope line: a sender followed by
/// an `asctime` date (`Thu Oct  1 10:00:00 2026`), possibly with a zone.
pub fn looks_like_envelope(line: &[u8]) -> bool {
    let Ok(text) = std::str::from_utf8(line) else {
        return false;
    };
    let Some(rest) = text.strip_prefix("From ") else {
        return false;
    };
    envelope_date(rest.trim_end()).is_some()
}

const DAYS: [&str; 7] = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

/// The date of an envelope (`sender Thu Oct  1 10:00:00 2026`), read as
/// UTC (a numeric zone after the time or year is applied).
pub fn envelope_date(envelope: &str) -> Option<i64> {
    let tokens: Vec<&str> = envelope.split_whitespace().collect();
    let start = tokens.iter().position(|t| DAYS.contains(t))?;
    let rest = &tokens[start..];
    if rest.len() < 5 {
        return None;
    }
    let (time_index, year_index) = (
        3,
        if rest[4].starts_with(['+', '-']) {
            5
        } else {
            4
        },
    );
    let zone = rest.iter().skip(4).find(|t| {
        t.len() == 5 && t.starts_with(['+', '-']) && t[1..].bytes().all(|b| b.is_ascii_digit())
    });
    let year = rest.get(year_index)?;
    let text = format!(
        "{} {} {} {} {}",
        rest[0], rest[1], rest[2], rest[time_index], year
    );
    let naive = NaiveDateTime::parse_from_str(&text, "%a %b %e %H:%M:%S %Y")
        .or_else(|_| NaiveDateTime::parse_from_str(&text, "%a %b %e %H:%M %Y"))
        .ok()?;
    let mut seconds = naive.and_utc().timestamp();
    if let Some(zone) = zone {
        let sign = if zone.starts_with('-') { -1 } else { 1 };
        let hours: i64 = zone[1..3].parse().ok()?;
        let minutes: i64 = zone[3..5].parse().ok()?;
        seconds -= sign * (hours * 3600 + minutes * 60);
    }
    Some(seconds)
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn read_all(data: &[u8], options: MboxReadOptions) -> Vec<MboxEntry> {
        MboxReader::new(data, options)
            .collect::<io::Result<Vec<_>>>()
            .unwrap()
    }

    fn texts(entries: &[MboxEntry]) -> Vec<String> {
        entries
            .iter()
            .map(|e| String::from_utf8(e.bytes.clone()).unwrap())
            .collect()
    }

    #[test]
    fn envelope_dates() {
        assert_eq!(
            envelope_date("ada@example.org Thu Oct  1 10:00:00 2026"),
            Some(1_790_848_800)
        );
        assert_eq!(
            envelope_date("MAILER-DAEMON Thu Oct 1 12:00:00 +0200 2026"),
            Some(1_790_848_800)
        );
        assert_eq!(
            envelope_date("- Thu Oct 01 10:00:00 2026 +0000"),
            Some(1_790_848_800)
        );
        assert_eq!(envelope_date("someone yesterday"), None);
        assert!(looks_like_envelope(b"From a@b Thu Oct  1 10:00:00 2026\n"));
        assert!(!looks_like_envelope(b"From here on, we talk.\n"));
    }

    #[test]
    fn splits_messages_and_unquotes_mboxrd() {
        let data = b"From a@b Thu Oct  1 10:00:00 2026\nSubject: one\n\nHello\n>From the start\n>>From quoted\n\nFrom b@c Thu Oct  1 11:00:00 2026\nSubject: two\n\nBye\n\n";
        let entries = read_all(data, MboxReadOptions::default());
        assert_eq!(entries.len(), 2);
        assert_eq!(
            texts(&entries),
            vec![
                "Subject: one\n\nHello\nFrom the start\n>From quoted\n",
                "Subject: two\n\nBye\n"
            ]
        );
        assert_eq!(entries[0].index, 1);
        assert_eq!(entries[0].offset, 0);
        assert_eq!(entries[1].offset, 85);
        assert_eq!(&data[85..90], b"From ");
        assert_eq!(entries[0].dialect, Some(MboxDialect::Mboxrd));
        assert_eq!(entries[0].envelope_sender(), Some("a@b"));
        assert_eq!(entries[0].received_at(), Some(1_790_848_800));
        assert!(!entries[1].truncated);
    }

    #[test]
    fn mboxo_keeps_double_quotes() {
        let data = b"From a Thu Oct  1 10:00:00 2026\nS: x\n\n>From a\n>>From b\n";
        let options = MboxReadOptions {
            dialect: Some(MboxDialect::Mboxo),
            ..MboxReadOptions::default()
        };
        assert_eq!(
            texts(&read_all(data, options)),
            vec!["S: x\n\nFrom a\n>>From b\n"]
        );
    }

    #[test]
    fn crlf_files() {
        let data = b"From a Thu Oct  1 10:00:00 2026\r\nS: x\r\n\r\nbody\r\n\r\nFrom b Thu Oct  1 10:00:00 2026\r\nS: y\r\n\r\nz\r\n";
        assert_eq!(
            texts(&read_all(data, MboxReadOptions::default())),
            vec!["S: x\r\n\r\nbody\r\n", "S: y\r\n\r\nz\r\n"]
        );
    }

    #[test]
    fn from_lines_without_a_blank_line_need_a_date() {
        let data = b"From a Thu Oct  1 10:00:00 2026\nS: x\n\nline\nFrom here on.\nFrom b Thu Oct  1 11:00:00 2026\nS: y\n\nz\n";
        let entries = read_all(data, MboxReadOptions::default());
        assert_eq!(
            texts(&entries),
            vec!["S: x\n\nline\nFrom here on.\n", "S: y\n\nz\n"]
        );
    }

    #[test]
    fn content_length_bodies_keep_from_lines() {
        let body = "line\n\nFrom inside the body\nend\n";
        let data = format!(
            "From a Thu Oct  1 10:00:00 2026\nContent-Length: {}\nS: x\n\n{body}\nFrom b Thu Oct  1 10:00:00 2026\nS: y\n\nz\n",
            body.len()
        );
        let entries = read_all(data.as_bytes(), MboxReadOptions::default());
        assert_eq!(entries.len(), 2);
        assert_eq!(
            texts(&entries)[0],
            format!("Content-Length: {}\nS: x\n\n{body}", body.len())
        );
        assert_eq!(entries[0].dialect, Some(MboxDialect::Mboxcl2));
        assert_eq!(texts(&entries)[1], "S: y\n\nz\n");
    }

    #[test]
    fn mboxcl_unquotes() {
        let body = ">From quoted\n";
        let data = format!(
            "From a Thu Oct  1 10:00:00 2026\nContent-Length: {}\n\n{body}",
            body.len()
        );
        let entries = read_all(data.as_bytes(), MboxReadOptions::default());
        assert_eq!(entries[0].dialect, Some(MboxDialect::Mboxcl));
        assert_eq!(
            texts(&entries)[0],
            format!("Content-Length: {}\n\nFrom quoted\n", body.len())
        );
    }

    #[test]
    fn wrong_content_length_falls_back() {
        let data = b"From a Thu Oct  1 10:00:00 2026\nContent-Length: 3\nS: x\n\nlonger body\n\nFrom b Thu Oct  1 10:00:00 2026\nS: y\n\nz\n";
        let entries = read_all(data, MboxReadOptions::default());
        assert_eq!(
            texts(&entries),
            vec!["Content-Length: 3\nS: x\n\nlonger body\n", "S: y\n\nz\n"]
        );
        let long = b"From a Thu Oct  1 10:00:00 2026\nContent-Length: 999\nS: x\n\nshort\n\nFrom b Thu Oct  1 10:00:00 2026\nS: y\n\nz\n";
        let entries = read_all(long, MboxReadOptions::default());
        assert_eq!(entries.len(), 2);
        assert!(!entries[0].truncated);
    }

    #[test]
    fn short_content_length_at_the_end_is_truncated() {
        let data = b"From a Thu Oct  1 10:00:00 2026\nContent-Length: 100\n\nonly this\n";
        let entries = read_all(data, MboxReadOptions::default());
        assert_eq!(entries.len(), 1);
        assert!(entries[0].truncated);
        assert_eq!(texts(&entries)[0], "Content-Length: 100\n\nonly this\n");
    }

    #[test]
    fn unfinished_headers_are_truncated() {
        let data = b"From a Thu Oct  1 10:00:00 2026\nS: x\n\nok\n\nFrom b Thu Oct  1 10:00:00 2026\nSubject: cut";
        let entries = read_all(data, MboxReadOptions::default());
        assert_eq!(entries.len(), 2);
        assert!(!entries[0].truncated);
        assert!(entries[1].truncated && entries[1].headers_unfinished);
    }

    #[test]
    fn too_large_messages_are_skipped_without_keeping_them() {
        let big = "x".repeat(5000);
        let data = format!(
            "From a Thu Oct  1 10:00:00 2026\nS: big\n\n{big}\n\nFrom b Thu Oct  1 10:00:00 2026\nS: small\n\nok\n"
        );
        let options = MboxReadOptions {
            max_message_bytes: 1000,
            ..MboxReadOptions::default()
        };
        let entries = read_all(data.as_bytes(), options);
        assert!(entries[0].too_large);
        assert!(entries[0].bytes.is_empty());
        assert_eq!(entries[0].size_on_disk, (8 + 5001) as u64);
        assert!(!entries[1].too_large);
        assert_eq!(texts(&entries)[1], "S: small\n\nok\n");
    }

    #[test]
    fn long_lines_are_read_in_pieces() {
        let long = "y".repeat(MAX_LINE_CHUNK * 2 + 17);
        let data = format!(
            "From a Thu Oct  1 10:00:00 2026\nS: x\n\n{long}\nFrom notasep\n\nFrom b Thu Oct  1 10:00:00 2026\nS: y\n\nz\n"
        );
        let entries = read_all(data.as_bytes(), MboxReadOptions::default());
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].bytes.len(), 6 + long.len() + 1 + 13);
    }

    /// Generates an mbox on the fly: the reader never sees the whole file.
    struct Generated {
        messages: u64,
        next: u64,
        pending: Vec<u8>,
        position: usize,
        big_at: u64,
    }

    impl io::Read for Generated {
        fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
            if self.position == self.pending.len() {
                if self.next == self.messages {
                    return Ok(0);
                }
                self.next += 1;
                let body = if self.next == self.big_at {
                    "z".repeat(3 * 1024 * 1024)
                } else {
                    format!("message {}\nFrom inside", self.next)
                };
                self.pending = format!(
                    "From x@y Thu Oct  1 10:00:00 2026\nSubject: {}\n\n{}\n\n",
                    self.next,
                    body.replace("\nFrom ", "\n>From ")
                )
                .into_bytes();
                self.position = 0;
            }
            let n = out.len().min(self.pending.len() - self.position);
            out[..n].copy_from_slice(&self.pending[self.position..self.position + n]);
            self.position += n;
            Ok(n)
        }
    }

    #[test]
    fn streams_large_files() {
        let source = Generated {
            messages: 50_000,
            next: 0,
            pending: Vec::new(),
            position: 0,
            big_at: 25_000,
        };
        let options = MboxReadOptions {
            max_message_bytes: 1024 * 1024,
            ..MboxReadOptions::default()
        };
        let mut reader = MboxReader::new(io::BufReader::with_capacity(64 * 1024, source), options);
        let mut count = 0u64;
        let mut largest_kept = 0usize;
        let mut too_large = Vec::new();
        while let Some(entry) = reader.next_entry().unwrap() {
            count += 1;
            largest_kept = largest_kept.max(entry.bytes.len());
            if entry.too_large {
                too_large.push(entry.index);
            } else if entry.index == 7 {
                assert_eq!(entry.bytes, b"Subject: 7\n\nmessage 7\nFrom inside\n");
            }
        }
        assert_eq!(count, 50_000);
        assert_eq!(too_large, vec![25_000]);
        // Nothing near the big message was held.
        assert!(largest_kept < 100, "{largest_kept}");
        assert!(reader.bytes_read() > 3 * 1024 * 1024);
    }

    #[test]
    fn counting_only() {
        let data = b"From a Thu Oct  1 10:00:00 2026\nS: x\n\nbody\n";
        let options = MboxReadOptions {
            keep_bytes: false,
            ..MboxReadOptions::default()
        };
        let entries = read_all(data, options);
        assert_eq!(entries.len(), 1);
        assert!(entries[0].bytes.is_empty());
        assert_eq!(entries[0].size_on_disk, 11);
    }

    #[test]
    fn not_an_mbox() {
        let mut reader = MboxReader::new(&b"Subject: x\n\nbody"[..], MboxReadOptions::default());
        let error = reader.next_entry().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::InvalidData);
        let mut empty = MboxReader::new(&b"\n\n"[..], MboxReadOptions::default());
        assert!(empty.next_entry().unwrap().is_none());
    }

    #[test]
    fn headers_only_message_before_a_from_line() {
        let data =
            b"From a Thu Oct  1 10:00:00 2026\nS: x\nFrom b Thu Oct  1 10:00:00 2026\nS: y\n\nz\n";
        let entries = read_all(data, MboxReadOptions::default());
        assert_eq!(texts(&entries), vec!["S: x\n", "S: y\n\nz\n"]);
    }
}
