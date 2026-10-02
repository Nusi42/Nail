//! mbox files: the streaming reader ([`read`]) and the writer.

pub mod read;

use std::io::{self, Write};

use chrono::{DateTime, Utc};
use nail_api::portability::MboxVariant;

pub use read::{MboxDialect, MboxEntry, MboxReadOptions, MboxReader};

use crate::message::{header_facts, header_fields, header_value};
use crate::sink::ExportMessage;

/// Writes messages as an mbox file.
pub struct MboxWriter<W> {
    out: W,
    variant: MboxVariant,
    messages: u64,
    bytes: u64,
}

impl<W: Write> MboxWriter<W> {
    /// A writer in `variant`.
    pub fn new(out: W, variant: MboxVariant) -> Self {
        Self {
            out,
            variant,
            messages: 0,
            bytes: 0,
        }
    }

    /// Messages written.
    pub fn messages(&self) -> u64 {
        self.messages
    }

    /// Bytes written.
    pub fn bytes(&self) -> u64 {
        self.bytes
    }

    /// Writes one message: the `From ` line (envelope sender, delivery time
    /// in `asctime` form, UTC), the quoted message, an empty line. `now` is
    /// used when the message has no time at all.
    pub fn write_message(&mut self, message: &ExportMessage, now: DateTime<Utc>) -> io::Result<()> {
        let raw = &message.raw;
        let crlf = uses_crlf(raw);
        let eol: &[u8] = if crlf { b"\r\n" } else { b"\n" };
        let from_line = format!(
            "From {} {}",
            envelope_sender(message),
            asctime(delivery_time(message).unwrap_or_else(|| now.timestamp()))
        );
        let mut written = 0u64;
        self.out.write_all(from_line.as_bytes())?;
        self.out.write_all(eol)?;
        written += (from_line.len() + eol.len()) as u64;
        let mut ends_with_newline = true;
        for line in raw.split_inclusive(|&b| b == b'\n') {
            if needs_quote(line, self.variant) {
                self.out.write_all(b">")?;
                written += 1;
            }
            self.out.write_all(line)?;
            written += line.len() as u64;
            ends_with_newline = line.ends_with(b"\n");
        }
        if !raw.is_empty() && !ends_with_newline {
            self.out.write_all(eol)?;
            written += eol.len() as u64;
        }
        self.out.write_all(eol)?;
        written += eol.len() as u64;
        self.bytes += written;
        self.messages += 1;
        Ok(())
    }

    /// Flushes and returns the inner writer.
    pub fn finish(mut self) -> io::Result<W> {
        self.out.flush()?;
        Ok(self.out)
    }
}

fn uses_crlf(raw: &[u8]) -> bool {
    raw.iter()
        .position(|&b| b == b'\n')
        .is_some_and(|i| i > 0 && raw[i - 1] == b'\r')
}

fn needs_quote(line: &[u8], variant: MboxVariant) -> bool {
    match variant {
        MboxVariant::Mboxo => line.starts_with(b"From "),
        MboxVariant::Mboxrd => {
            let quotes = line.iter().take_while(|&&b| b == b'>').count();
            line[quotes..].starts_with(b"From ")
        }
    }
}

/// The envelope sender for the `From ` line: the given one, else
/// `Return-Path`, else the `From` address, else `MAILER-DAEMON`; never with
/// spaces.
fn envelope_sender(message: &ExportMessage) -> String {
    let fields = header_fields(&message.raw);
    let candidate = message
        .envelope_from
        .clone()
        .or_else(|| header_value(&fields, "Return-Path").map(str::to_owned))
        .or_else(|| {
            header_value(&fields, "From").map(|from| match (from.rfind('<'), from.rfind('>')) {
                (Some(start), Some(end)) if start < end => from[start + 1..end].to_owned(),
                _ => from.to_owned(),
            })
        })
        .unwrap_or_default();
    let cleaned: String = candidate
        .trim()
        .trim_matches(['<', '>'])
        .chars()
        .filter(|c| !c.is_whitespace() && !c.is_control())
        .collect();
    if cleaned.is_empty() {
        "MAILER-DAEMON".to_owned()
    } else {
        cleaned
    }
}

fn delivery_time(message: &ExportMessage) -> Option<i64> {
    message
        .received_at
        .or_else(|| header_facts(&message.raw).date)
}

/// `Thu Oct  1 10:00:00 2026` (UTC).
pub fn asctime(timestamp: i64) -> String {
    DateTime::<Utc>::from_timestamp(timestamp, 0)
        .unwrap_or_default()
        .format("%a %b %e %H:%M:%S %Y")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    fn message(raw: &str) -> ExportMessage {
        ExportMessage {
            raw: raw.as_bytes().to_vec(),
            ..ExportMessage::default()
        }
    }

    fn now() -> DateTime<Utc> {
        DateTime::from_timestamp(1_790_848_800, 0).unwrap()
    }

    #[test]
    fn writes_mboxrd() {
        let mut writer = MboxWriter::new(Vec::new(), MboxVariant::Mboxrd);
        writer
            .write_message(
                &message("From: Ada <ada@example.org>\nSubject: x\n\nFrom me\n>From you\nbye"),
                now(),
            )
            .unwrap();
        assert_eq!(writer.messages(), 1);
        let bytes = writer.bytes();
        let out = String::from_utf8(writer.finish().unwrap()).unwrap();
        assert_eq!(
            out,
            "From ada@example.org Thu Oct  1 10:00:00 2026\nFrom: Ada <ada@example.org>\nSubject: x\n\n>From me\n>>From you\nbye\n\n"
        );
        assert_eq!(bytes, out.len() as u64);
    }

    #[test]
    fn writes_mboxo() {
        let mut writer = MboxWriter::new(Vec::new(), MboxVariant::Mboxo);
        writer
            .write_message(&message("S: x\n\nFrom me\n>From you\n"), now())
            .unwrap();
        let out = String::from_utf8(writer.finish().unwrap()).unwrap();
        assert_eq!(
            out,
            "From MAILER-DAEMON Thu Oct  1 10:00:00 2026\nS: x\n\n>From me\n>From you\n\n"
        );
    }

    #[test]
    fn senders_and_times() {
        let mut m = message(
            "Return-Path: <bounce@example.org>\nDate: Fri, 02 Oct 2026 08:00:00 +0200\n\nx\n",
        );
        assert_eq!(envelope_sender(&m), "bounce@example.org");
        assert_eq!(delivery_time(&m), Some(1_790_920_800));
        m.envelope_from = Some("a b@c".into());
        m.received_at = Some(0);
        assert_eq!(envelope_sender(&m), "ab@c");
        assert_eq!(delivery_time(&m), Some(0));
        assert_eq!(asctime(0), "Thu Jan  1 00:00:00 1970");
    }

    #[test]
    fn crlf_messages_keep_their_line_ends_and_round_trip() {
        let originals = [
            "Subject: one\r\n\r\nFrom the start\r\n>From quoted\r\n",
            "Subject: two\r\n\r\n\r\nempty lines\r\n\r\n",
        ];
        let mut writer = MboxWriter::new(Vec::new(), MboxVariant::Mboxrd);
        for raw in originals {
            writer.write_message(&message(raw), now()).unwrap();
        }
        let out = writer.finish().unwrap();
        let back: Vec<String> = MboxReader::new(&out[..], MboxReadOptions::default())
            .map(|e| String::from_utf8(e.unwrap().bytes).unwrap())
            .collect();
        assert_eq!(back, originals);
    }
}
