//! The generated benchmark corpus (plan M4c WP123).
//!
//! [`Corpus`] turns a [`CorpusSpec`] into a deterministic stream of RFC 5322
//! messages: the same spec (and seed) always yields byte-identical
//! messages. Shape:
//!
//! * **Folders**: `INBOX`, `Sent`, `Archive`, `Junk`, then parents
//!   (`Projects`, `Lists`, `Clients`) and their children up to
//!   [`CorpusSpec::folders`]; the Inbox gets the largest share, the rest
//!   follow a Zipf-like curve.
//! * **People**: a pool of senders where a few write most of the mail.
//! * **Threads**: [`CorpusSpec::thread_ratio`] of the messages answer a
//!   recent message of the same folder (`In-Reply-To`, `References`,
//!   `Re:` subject).
//! * **Bodies**: plain text, or `multipart/alternative` with HTML for
//!   [`CorpusSpec::html_ratio`]; sizes vary around
//!   [`CorpusSpec::mean_body_bytes`].
//! * **Attachments**: [`CorpusSpec::attachment_ratio`] of the messages carry
//!   one or two base64 attachments of realistic names and sizes around
//!   [`CorpusSpec::mean_attachment_bytes`] (incompressible content).
//! * **Dates** spread over three years before [`CORPUS_END`], newest last;
//!   most older mail is read, some is flagged or answered.
//!
//! A few markers make search results predictable: [`RARE_WORD`] appears in
//! exactly one message, [`PHRASE`] in about one message in fifty.

use std::collections::VecDeque;
use std::fmt::Write as _;

use base64::Engine as _;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};

use crate::CorpusSpec;
use crate::num::{f64_of, f64_of_len, i64_of, usize_of};

/// The newest generated message is from this moment (Unix seconds,
/// 2026-09-30T18:00:00Z); the corpus spans the three years before.
pub const CORPUS_END: i64 = 1_790_791_200;

/// How far back the corpus reaches (seconds; three years).
pub const CORPUS_SPAN: i64 = 3 * 365 * 24 * 3600;

/// The account's own address (the `To` of received mail, the `From` of
/// sent mail).
pub const OWN_ADDRESS: &str = "sam@nail.example";

/// A word that occurs in exactly one message of every corpus.
pub const RARE_WORD: &str = "zyzzyva";

/// A phrase in about one message in fifty.
pub const PHRASE: &str = "quarterly report";

/// The fixed folders, before the generated ones: name and role.
const FIXED_FOLDERS: [(&str, Role); 4] = [
    ("INBOX", Role::Inbox),
    ("Sent", Role::Sent),
    ("Archive", Role::Archive),
    ("Junk", Role::Junk),
];

/// Parents of the generated folders.
const PARENTS: [&str; 3] = ["Projects", "Lists", "Clients"];

const PROJECTS: [&str; 16] = [
    "Apollo", "Borealis", "Cascade", "Delta", "Ember", "Fjord", "Granite", "Helios", "Iris",
    "Juniper", "Kestrel", "Lumen", "Meridian", "Nimbus", "Orion", "Polaris",
];

const LISTS: [&str; 12] = [
    "rust-users",
    "tokio",
    "sqlite",
    "kdl",
    "react",
    "typescript",
    "tauri",
    "security-announce",
    "fluent",
    "tantivy",
    "playwright",
    "windows-dev",
];

const CLIENTS: [&str; 12] = [
    "Contoso",
    "Fabrikam",
    "Northwind",
    "Tailspin",
    "Wingtip",
    "Litware",
    "Adatum",
    "Proseware",
    "Lucerne",
    "Coho",
    "Fourth Coffee",
    "Woodgrove",
];

const FIRST_NAMES: [&str; 40] = [
    "Ada",
    "Grace",
    "Alan",
    "Barbara",
    "Edsger",
    "Frances",
    "Donald",
    "Margaret",
    "Dennis",
    "Radia",
    "Ken",
    "Hedy",
    "Linus",
    "Katherine",
    "Tim",
    "Sophie",
    "John",
    "Mary",
    "Niklaus",
    "Karen",
    "Bjarne",
    "Lynn",
    "Guido",
    "Adele",
    "Brian",
    "Anita",
    "Leslie",
    "Jean",
    "Ivan",
    "Shafi",
    "Yukihiro",
    "Joan",
    "Larry",
    "Evelyn",
    "Rob",
    "Carol",
    "Andrew",
    "Fran",
    "Butler",
    "Susan",
];

const LAST_NAMES: [&str; 40] = [
    "Lovelace",
    "Hopper",
    "Turing",
    "Liskov",
    "Dijkstra",
    "Allen",
    "Knuth",
    "Hamilton",
    "Ritchie",
    "Perlman",
    "Thompson",
    "Lamarr",
    "Torvalds",
    "Johnson",
    "Berners",
    "Wilson",
    "Backus",
    "Keller",
    "Wirth", // spellchecker:disable-line
    "Jones",
    "Stroustrup",
    "Conway",
    "Rossum",
    "Goldberg",
    "Kernighan",
    "Borg",
    "Lamport",
    "Sammet",
    "Sutherland",
    "Goldwasser",
    "Matsumoto",
    "Clarke",
    "Page",
    "Boyd",
    "Pike",
    "Shaw",
    "Tanenbaum",
    "Bilas",
    "Lampson",
    "Kare",
];

const DOMAINS: [&str; 10] = [
    "example.org",
    "contoso.example",
    "fabrikam.example",
    "northwind.example",
    "lists.example.net",
    "mail.example.com",
    "uni.example.edu",
    "tailspin.example",
    "wingtip.example",
    "litware.example",
];

const TOPICS: &[&str] = &[
    "budget",
    "roadmap",
    "release",
    "invoice",
    "meeting",
    "review",
    "deadline",
    "contract",
    "proposal",
    "migration",
    "outage",
    "hiring",
    "offsite",
    "design",
    "security",
    "benchmark",
    "onboarding",
    "travel",
    "workshop",
    "feedback",
    "milestone",
    "planning",
    "estimate",
    "renewal",
    "incident",
    "launch",
    "survey",
    "training",
    "audit",
    "forecast",
    "pricing",
    "backlog",
    "retrospective",
    "architecture",
    "license",
    "deployment",
    "newsletter",
    "conference",
    "webinar",
    "partnership",
    "compliance",
    "inventory",
    "shipment",
    "quote",
    "schedule",
    "support",
    "upgrade",
    "integration",
];

const WORDS: &[&str] = &[
    "the",
    "and",
    "for",
    "with",
    "that",
    "this",
    "from",
    "have",
    "will",
    "about",
    "please",
    "thanks",
    "team",
    "update",
    "attached",
    "next",
    "week",
    "today",
    "tomorrow",
    "could",
    "would",
    "should",
    "change",
    "issue",
    "question",
    "answer",
    "data",
    "report",
    "numbers",
    "draft",
    "final",
    "version",
    "notes",
    "agenda",
    "call",
    "time",
    "project",
    "client",
    "customer",
    "server",
    "window",
    "message",
    "folder",
    "search",
    "index",
    "thread",
    "plan",
    "goal",
    "result",
    "test",
    "build",
    "deploy",
    "monday",
    "tuesday",
    "wednesday",
    "thursday",
    "friday",
    "morning",
    "afternoon",
    "office",
    "remote",
    "quick",
    "important",
    "follow",
    "up",
    "check",
    "confirm",
    "share",
    "slides",
    "document",
    "spreadsheet",
    "chart",
    "summary",
    "detail",
    "context",
    "decision",
    "approval",
    "risk",
    "cost",
    "value",
    "quality",
    "speed",
    "memory",
    "storage",
    "network",
    "latency",
    "throughput",
    "capacity",
    "growth",
    "users",
    "feature",
    "bug",
    "fix",
    "patch",
];

const ATTACHMENT_KINDS: [(&str, &str, &str); 8] = [
    ("report", "pdf", "application/pdf"),
    ("IMG", "jpg", "image/jpeg"),
    (
        "notes",
        "docx",
        "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
    ),
    (
        "figures",
        "xlsx",
        "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
    ),
    (
        "slides",
        "pptx",
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
    ),
    ("screenshot", "png", "image/png"),
    ("archive", "zip", "application/zip"),
    ("invite", "ics", "text/calendar"),
];

/// A folder's special use (mirrors `nail_api::FolderRole` without the
/// dependency on the API's full enum in the generator).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Role {
    /// The Inbox.
    Inbox,
    /// Sent mail.
    Sent,
    /// Archive.
    Archive,
    /// Junk.
    Junk,
    /// Any other folder.
    Other,
}

impl Role {
    /// The API's folder role.
    pub fn api(self) -> nail_api::FolderRole {
        match self {
            Self::Inbox => nail_api::FolderRole::Inbox,
            Self::Sent => nail_api::FolderRole::Sent,
            Self::Archive => nail_api::FolderRole::Archive,
            Self::Junk => nail_api::FolderRole::Junk,
            Self::Other => nail_api::FolderRole::Other,
        }
    }
}

/// A folder of the corpus.
#[derive(Debug, Clone, PartialEq)]
pub struct CorpusFolder {
    /// Full name, hierarchy delimiter `/`.
    pub name: String,
    /// Special use.
    pub role: Role,
    /// Share of the messages (weights sum to 1).
    pub weight: f64,
}

/// One generated message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GeneratedMessage {
    /// Position in the corpus (0 = oldest).
    pub index: u64,
    /// Index into [`Corpus::folders`].
    pub folder: usize,
    /// Raw RFC 5322 bytes, CRLF line endings.
    pub raw: Vec<u8>,
    /// IMAP flags (`\Seen`, `\Flagged`, `\Answered`).
    pub flags: Vec<&'static str>,
    /// Receive time (Unix seconds).
    pub date: i64,
    /// Whether it carries an attachment.
    pub has_attachment: bool,
    /// Whether it answers an earlier message.
    pub is_reply: bool,
}

impl GeneratedMessage {
    /// The IMAP `INTERNALDATE` (`28-Sep-2026 08:15:00 +0000`).
    pub fn internal_date(&self) -> String {
        chrono::DateTime::from_timestamp(self.date, 0)
            .unwrap_or_default()
            .format("%d-%b-%Y %H:%M:%S +0000")
            .to_string()
    }
}

/// A person of the sender pool.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    /// Display name.
    pub name: String,
    /// Address.
    pub address: String,
}

/// The folders and people of a corpus plus the message stream.
#[derive(Debug, Clone)]
pub struct Corpus {
    spec: CorpusSpec,
    folders: Vec<CorpusFolder>,
    people: Vec<Person>,
}

/// Summary counts of a generated corpus.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CorpusStats {
    /// Messages.
    pub messages: u64,
    /// Total raw bytes.
    pub bytes: u64,
    /// Messages with an attachment.
    pub with_attachment: u64,
    /// Replies.
    pub replies: u64,
    /// Messages per folder (by folder index).
    pub per_folder: Vec<u64>,
    /// Unread messages.
    pub unread: u64,
}

impl Corpus {
    /// The corpus of `spec` (folders and people; messages are generated on
    /// demand by [`Self::messages`]).
    pub fn new(spec: CorpusSpec) -> Self {
        let folders = folder_layout(spec.folders.max(1) as usize);
        let people = people(spec.seed);
        Self {
            spec,
            folders,
            people,
        }
    }

    /// The spec.
    pub fn spec(&self) -> &CorpusSpec {
        &self.spec
    }

    /// The folders, in creation order (parents before children).
    pub fn folders(&self) -> &[CorpusFolder] {
        &self.folders
    }

    /// The sender pool, most active first.
    pub fn people(&self) -> &[Person] {
        &self.people
    }

    /// Every message, oldest first.
    pub fn messages(&self) -> MessageStream<'_> {
        MessageStream::new(self, self.spec.seed, 0, self.spec.messages, None)
    }

    /// `count` new messages for the Inbox that arrive after the corpus
    /// (incremental sync), numbered from `first` (call again with the next
    /// number for another batch). Deterministic as well.
    pub fn arrivals(&self, first: u64, count: u64) -> MessageStream<'_> {
        MessageStream::new(
            self,
            self.spec.seed ^ 0x00D3_11E0_A1A1 ^ first.wrapping_mul(0x9E37_79B9_7F4A_7C15),
            self.spec.messages + first,
            count,
            Some(0),
        )
    }

    /// Counts the corpus without keeping it.
    pub fn stats(&self) -> CorpusStats {
        let mut stats = CorpusStats {
            per_folder: vec![0; self.folders.len()],
            ..CorpusStats::default()
        };
        for m in self.messages() {
            stats.messages += 1;
            stats.bytes += m.raw.len() as u64;
            stats.with_attachment += u64::from(m.has_attachment);
            stats.replies += u64::from(m.is_reply);
            stats.unread += u64::from(!m.flags.contains(&"\\Seen"));
            stats.per_folder[m.folder] += 1;
        }
        stats
    }

    /// Typical search queries for this corpus (what people type: a sender,
    /// a subject word, a body word, a phrase, attachments, unread mail,
    /// combinations and a word that occurs once).
    pub fn search_queries(&self) -> Vec<String> {
        let first = &self.people[0];
        let second = &self.people[1];
        let last_name = |p: &Person| {
            p.name
                .split_whitespace()
                .last()
                .unwrap_or_default()
                .to_ascii_lowercase()
        };
        vec![
            format!("from:{}", last_name(first)),
            "subject:budget".to_owned(),
            "invoice".to_owned(),
            format!("\"{PHRASE}\""),
            format!("has:attachment from:{}", last_name(second)),
            "is:unread".to_owned(),
            "filename:report".to_owned(),
            RARE_WORD.to_owned(),
            format!("from:{} OR from:{}", last_name(first), last_name(second)),
            "migration -is:read".to_owned(),
            "subject:release has:attachment".to_owned(),
            "is:flagged".to_owned(),
        ]
    }
}

/// The folder layout for `count` folders.
fn folder_layout(count: usize) -> Vec<CorpusFolder> {
    let mut names: Vec<(String, Role)> = FIXED_FOLDERS
        .iter()
        .take(count)
        .map(|(n, r)| ((*n).to_owned(), *r))
        .collect();
    let mut children: Vec<String> = Vec::new();
    let longest = PROJECTS.len().max(LISTS.len()).max(CLIENTS.len());
    for i in 0..longest {
        if let Some(p) = PROJECTS.get(i) {
            children.push(format!("Projects/{p}"));
        }
        if let Some(l) = LISTS.get(i) {
            children.push(format!("Lists/{l}"));
        }
        if let Some(c) = CLIENTS.get(i) {
            children.push(format!("Clients/{c}"));
        }
    }
    let mut child = 0usize;
    while names.len() < count {
        // A parent comes right before its first child; without room for
        // the parent the child stays at the top level.
        let mut next = children.get(child).cloned().unwrap_or_else(|| {
            let n = child - children.len();
            format!("Archive/{}", 2000 + n)
        });
        if let Some((parent, _)) = next.split_once('/')
            && !names.iter().any(|(n, _)| n == parent)
        {
            if PARENTS.contains(&parent) && names.len() + 1 < count {
                names.push((parent.to_owned(), Role::Other));
            } else {
                next = next.replace('/', "-");
            }
        }
        names.push((next, Role::Other));
        child += 1;
    }
    // Weights: Inbox 30 %, Sent 12 %, Archive 18 %, Junk 2 %, the rest share
    // 38 % on a Zipf curve (parents get little: people file into leaves).
    let fixed = [0.30, 0.12, 0.18, 0.02];
    let rest = count.saturating_sub(FIXED_FOLDERS.len());
    let zipf: Vec<f64> = (0..rest)
        .map(|k| {
            let name = &names[FIXED_FOLDERS.len() + k].0;
            let base = 1.0 / (f64_of_len(k) + 2.0);
            if PARENTS.contains(&name.as_str()) {
                base * 0.1
            } else {
                base
            }
        })
        .collect();
    let zipf_sum: f64 = zipf.iter().sum();
    let fixed_sum: f64 = fixed.iter().take(count.min(4)).sum();
    let rest_share = if rest == 0 { 0.0 } else { 0.38 };
    let total = fixed_sum + rest_share;
    names
        .into_iter()
        .enumerate()
        .map(|(i, (name, role))| {
            let weight = if i < FIXED_FOLDERS.len() {
                fixed[i] / total
            } else {
                rest_share * zipf[i - FIXED_FOLDERS.len()] / zipf_sum / total
            };
            CorpusFolder { name, role, weight }
        })
        .collect()
}

/// The sender pool: 400 people, ordered so the first few are the most
/// active (the generator picks them on a Zipf curve).
fn people(seed: u64) -> Vec<Person> {
    let mut rng = StdRng::seed_from_u64(seed ^ 0x0050_4550_4C45);
    (0..400)
        .map(|i| {
            let first =
                FIRST_NAMES[(i * 7 + rng.gen_range(0..FIRST_NAMES.len())) % FIRST_NAMES.len()];
            let last = LAST_NAMES[i % LAST_NAMES.len()];
            let domain = DOMAINS[rng.gen_range(0..DOMAINS.len())];
            let suffix = if i < LAST_NAMES.len() {
                String::new()
            } else {
                (i / LAST_NAMES.len()).to_string()
            };
            Person {
                name: format!("{first} {last}"),
                address: format!(
                    "{}.{}{suffix}@{domain}",
                    first.to_ascii_lowercase(),
                    last.to_ascii_lowercase()
                ),
            }
        })
        .collect()
}

/// A message a reply can answer.
#[derive(Debug, Clone)]
struct Root {
    message_id: String,
    references: String,
    subject: String,
    person: usize,
}

/// The message iterator of [`Corpus::messages`] / [`Corpus::arrivals`].
#[derive(Debug)]
pub struct MessageStream<'a> {
    corpus: &'a Corpus,
    rng: StdRng,
    next: u64,
    end: u64,
    first_index: u64,
    total: u64,
    fixed_folder: Option<usize>,
    cumulative: Vec<f64>,
    recent: Vec<VecDeque<Root>>,
    rare_at: u64,
}

impl<'a> MessageStream<'a> {
    fn new(
        corpus: &'a Corpus,
        seed: u64,
        first_index: u64,
        count: u64,
        fixed_folder: Option<usize>,
    ) -> Self {
        let mut acc = 0.0;
        let cumulative = corpus
            .folders
            .iter()
            .map(|f| {
                acc += f.weight;
                acc
            })
            .collect();
        let total = corpus.spec.messages.max(1);
        Self {
            corpus,
            rng: StdRng::seed_from_u64(seed),
            next: first_index,
            end: first_index + count,
            first_index,
            total,
            fixed_folder,
            cumulative,
            recent: vec![VecDeque::new(); corpus.folders.len()],
            // The one message with the rare word: in the middle of the
            // corpus (never in arrivals).
            rare_at: if fixed_folder.is_none() {
                total / 2
            } else {
                u64::MAX
            },
        }
    }

    fn pick_folder(&mut self) -> usize {
        if let Some(f) = self.fixed_folder {
            return f;
        }
        let x: f64 = self.rng.r#gen();
        self.cumulative
            .iter()
            .position(|c| x < *c)
            .unwrap_or(self.cumulative.len() - 1)
    }

    /// A person on a Zipf-like curve (a few people write most mail).
    fn pick_person(&mut self) -> usize {
        let n = self.corpus.people.len();
        let u: f64 = self.rng.r#gen();
        // Inverse of a power law: index ~ n * u^3.
        usize_of((u * u * u) * f64_of_len(n)) % n
    }

    fn words(&mut self, target: usize, out: &mut String) {
        let start = out.len();
        let mut sentence = 0;
        while out.len() - start < target {
            let w = WORDS[self.rng.gen_range(0..WORDS.len())];
            if sentence == 0 {
                let mut c = w.chars();
                if let Some(f) = c.next() {
                    out.extend(f.to_uppercase());
                    out.push_str(c.as_str());
                }
            } else {
                out.push_str(w);
            }
            sentence += 1;
            if self.rng.gen_bool(0.08) {
                out.push_str(TOPICS[self.rng.gen_range(0..TOPICS.len())]);
                out.push(' ');
                sentence += 1;
            }
            if sentence > 8 && self.rng.gen_bool(0.2) {
                out.push_str(". ");
                sentence = 0;
            } else {
                out.push(' ');
            }
        }
        out.push('.');
    }

    fn body_text(&mut self, size: usize, extra: &[&str]) -> Vec<String> {
        let mut paragraphs = Vec::new();
        let mut written = 0;
        while written < size {
            let mut p = String::new();
            let len = self.rng.gen_range(200..700).min(size - written + 40);
            self.words(len, &mut p);
            written += p.len() + 2;
            paragraphs.push(p);
        }
        for (i, e) in extra.iter().enumerate() {
            if let Some(p) = paragraphs.get_mut(i) {
                p.push(' ');
                p.push_str(e);
                p.push('.');
            }
        }
        paragraphs
    }

    fn generate(&mut self, index: u64) -> GeneratedMessage {
        let folder = self.pick_folder();
        let role = self.corpus.folders[folder].role;
        let position = f64_of(index - self.first_index.min(index));
        // Dates: spread over the span, newest last; arrivals come after.
        let date = if self.fixed_folder.is_some() {
            CORPUS_END
                + 60
                + i64::try_from(index - self.corpus.spec.messages)
                    .unwrap_or(i64::MAX)
                    .saturating_mul(30)
        } else {
            let base = CORPUS_END - CORPUS_SPAN
                + i64_of(f64_of(CORPUS_SPAN.unsigned_abs()) * (position / f64_of(self.total)));
            base + self.rng.gen_range(0..3600)
        };
        let person = self.pick_person();
        let reply_to = if self
            .rng
            .gen_bool(self.corpus.spec.thread_ratio.clamp(0.0, 1.0))
        {
            let recent = &self.recent[folder];
            (!recent.is_empty()).then(|| recent[self.rng.gen_range(0..recent.len())].clone())
        } else {
            None
        };
        let message_id = format!("<bench-{}-{index}@nail.example>", self.corpus.spec.seed);
        let subject = if let Some(root) = &reply_to {
            format!("Re: {}", root.subject.trim_start_matches("Re: "))
        } else {
            let topic = TOPICS[self.rng.gen_range(0..TOPICS.len())];
            let other = TOPICS[self.rng.gen_range(0..TOPICS.len())];
            let project = PROJECTS[self.rng.gen_range(0..PROJECTS.len())];
            match self.rng.gen_range(0..4) {
                0 => format!("{project} {topic} for week {}", index % 52 + 1),
                1 => format!("Question about the {topic}"),
                2 => format!("{topic} and {other} update"),
                _ => format!("[{project}] {topic} {other}"),
            }
        };
        let sender = &self.corpus.people[reply_to.as_ref().map_or(person, |r| {
            if self.rng.gen_bool(0.5) {
                r.person
            } else {
                person
            }
        })];
        let (from, to) = if role == Role::Sent {
            (
                format!("Sam Example <{OWN_ADDRESS}>"),
                format!("{} <{}>", sender.name, sender.address),
            )
        } else {
            (
                format!("{} <{}>", sender.name, sender.address),
                format!("Sam Example <{OWN_ADDRESS}>"),
            )
        };
        // Body size: exponential around the mean, clamped.
        let mean = f64::from(self.corpus.spec.mean_body_bytes.max(100));
        let u: f64 = self.rng.gen_range(0.0001..1.0);
        let size = usize_of((-u.ln() * mean).clamp(mean * 0.1, mean * 12.0));
        let mut extra: Vec<&str> = Vec::new();
        if index == self.rare_at {
            extra.push(RARE_WORD);
        }
        if self.rng.gen_bool(0.02) {
            extra.push("see the quarterly report");
        }
        let paragraphs = self.body_text(size, &extra);
        let html = self
            .rng
            .gen_bool(self.corpus.spec.html_ratio.clamp(0.0, 1.0));
        let has_attachment = self
            .rng
            .gen_bool(self.corpus.spec.attachment_ratio.clamp(0.0, 1.0));
        let mut attachments: Vec<(String, &'static str, Vec<u8>)> = Vec::new();
        if has_attachment {
            let count = if self.rng.gen_bool(0.2) { 2 } else { 1 };
            for n in 0..count {
                let (stem, ext, mime) =
                    ATTACHMENT_KINDS[self.rng.gen_range(0..ATTACHMENT_KINDS.len())];
                let amean = f64::from(self.corpus.spec.mean_attachment_bytes.max(1_000));
                let u: f64 = self.rng.gen_range(0.0001..1.0);
                // Log-normal-ish: most attachments are small, a few large.
                let len = usize_of(((-u.ln()).powf(1.6) * amean * 0.7).clamp(800.0, amean * 25.0));
                let mut bytes = vec![0u8; len];
                self.rng.fill(&mut bytes[..]);
                attachments.push((format!("{stem}-{}-{n}.{ext}", index % 10_000), mime, bytes));
            }
        }

        let mut raw = String::with_capacity(size * 2 + 1024);
        let date_text = chrono::DateTime::from_timestamp(date, 0)
            .unwrap_or_default()
            .format("%a, %d %b %Y %H:%M:%S +0000")
            .to_string();
        let _ = write!(
            raw,
            "Received: from mx{n}.example.net (mx{n}.example.net [192.0.2.{n}])\r\n\tby mx.nail.example with ESMTPS id {index:x}\r\n\tfor <{OWN_ADDRESS}>; {date_text}\r\n",
            n = index % 200 + 1
        );
        let _ = write!(
            raw,
            "Return-Path: <{}>\r\nFrom: {from}\r\nTo: {to}\r\n",
            sender.address
        );
        if self.rng.gen_bool(0.25) {
            let cc = &self.corpus.people[self.rng.gen_range(0..self.corpus.people.len())];
            let _ = write!(raw, "Cc: {} <{}>\r\n", cc.name, cc.address);
        }
        let _ = write!(
            raw,
            "Subject: {subject}\r\nDate: {date_text}\r\nMessage-ID: {message_id}\r\n"
        );
        let mut references = String::new();
        if let Some(root) = &reply_to {
            references = if root.references.is_empty() {
                root.message_id.clone()
            } else {
                // Keep the header bounded like real clients do.
                let mut refs: Vec<&str> = root.references.split(' ').collect();
                if refs.len() > 10 {
                    refs.drain(1..refs.len() - 9);
                }
                format!("{} {}", refs.join(" "), root.message_id)
            };
            let _ = write!(
                raw,
                "In-Reply-To: {}\r\nReferences: {references}\r\n",
                root.message_id
            );
        }
        if self.corpus.folders[folder].name.starts_with("Lists/") {
            let list = self.corpus.folders[folder]
                .name
                .trim_start_matches("Lists/")
                .to_owned();
            let _ = write!(
                raw,
                "List-Id: <{list}.lists.example.net>\r\nList-Unsubscribe: <mailto:{list}-leave@lists.example.net>\r\nPrecedence: list\r\n"
            );
        }
        let _ = write!(
            raw,
            "X-Mailer: Nail benchmark corpus\r\nDKIM-Signature: v=1; a=rsa-sha256; d={}; s=sel; h=from:to:subject:date; bh={}; b={}\r\nMIME-Version: 1.0\r\n",
            sender.address.split('@').nth(1).unwrap_or("example.org"),
            base64::engine::general_purpose::STANDARD.encode(index.to_be_bytes()),
            base64::engine::general_purpose::STANDARD.encode((index ^ 0xA5A5).to_le_bytes())
        );

        let text_part = |raw: &mut String| {
            raw.push_str("Content-Type: text/plain; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n");
            for p in &paragraphs {
                wrap(p, raw);
                raw.push_str("\r\n");
            }
        };
        let html_part = |raw: &mut String| {
            raw.push_str(
                "Content-Type: text/html; charset=utf-8\r\nContent-Transfer-Encoding: 8bit\r\n\r\n",
            );
            raw.push_str("<!DOCTYPE html><html><head><meta charset=\"utf-8\"><style>body{font-family:Segoe UI,sans-serif}p{margin:0 0 1em}</style></head><body>\r\n");
            for p in &paragraphs {
                raw.push_str("<p>");
                wrap(p, raw);
                raw.push_str("</p>\r\n");
            }
            raw.push_str("</body></html>\r\n");
        };
        let alt_boundary = format!("alt-{index:x}");
        let alternative = |raw: &mut String| {
            let _ = write!(
                raw,
                "Content-Type: multipart/alternative; boundary=\"{alt_boundary}\"\r\n\r\n--{alt_boundary}\r\n"
            );
            text_part(raw);
            let _ = write!(raw, "--{alt_boundary}\r\n");
            html_part(raw);
            let _ = write!(raw, "--{alt_boundary}--\r\n");
        };
        if attachments.is_empty() {
            if html {
                alternative(&mut raw);
            } else {
                text_part(&mut raw);
            }
        } else {
            let mixed = format!("mixed-{index:x}");
            let _ = write!(
                raw,
                "Content-Type: multipart/mixed; boundary=\"{mixed}\"\r\n\r\n--{mixed}\r\n"
            );
            if html {
                alternative(&mut raw);
            } else {
                text_part(&mut raw);
            }
            for (name, mime, bytes) in &attachments {
                let _ = write!(
                    raw,
                    "--{mixed}\r\nContent-Type: {mime}; name=\"{name}\"\r\nContent-Disposition: attachment; filename=\"{name}\"\r\nContent-Transfer-Encoding: base64\r\n\r\n"
                );
                let encoded = base64::engine::general_purpose::STANDARD.encode(bytes);
                for line in encoded.as_bytes().chunks(76) {
                    raw.push_str(std::str::from_utf8(line).unwrap_or_default());
                    raw.push_str("\r\n");
                }
            }
            let _ = write!(raw, "--{mixed}--\r\n");
        }

        // Flags: older mail is mostly read; Sent is read.
        let age = 1.0 - position / f64_of(self.total);
        let mut flags = Vec::new();
        let read = role == Role::Sent
            || (self.fixed_folder.is_none() && self.rng.gen_bool((0.6 + age * 0.39).min(0.995)));
        if read {
            flags.push("\\Seen");
        }
        if self.rng.gen_bool(0.03) {
            flags.push("\\Flagged");
        }
        if role == Role::Inbox && read && self.rng.gen_bool(0.2) {
            flags.push("\\Answered");
        }

        let recent = &mut self.recent[folder];
        recent.push_back(Root {
            message_id,
            references,
            subject,
            person,
        });
        if recent.len() > 64 {
            recent.pop_front();
        }
        GeneratedMessage {
            index,
            folder,
            raw: raw.into_bytes(),
            flags,
            date,
            has_attachment,
            is_reply: reply_to.is_some(),
        }
    }
}

impl Iterator for MessageStream<'_> {
    type Item = GeneratedMessage;

    fn next(&mut self) -> Option<Self::Item> {
        if self.next >= self.end {
            return None;
        }
        let index = self.next;
        self.next += 1;
        Some(self.generate(index))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let left = usize::try_from(self.end - self.next).unwrap_or(usize::MAX);
        (left, Some(left))
    }
}

/// Appends `text` wrapped at about 76 characters with CRLF line breaks.
fn wrap(text: &str, out: &mut String) {
    let mut line = 0;
    for word in text.split(' ') {
        if line > 0 && line + word.len() > 76 {
            out.push_str("\r\n");
            line = 0;
        } else if line > 0 {
            out.push(' ');
            line += 1;
        }
        out.push_str(word);
        line += word.len();
    }
    out.push_str("\r\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tiny() -> CorpusSpec {
        CorpusSpec {
            messages: 400,
            folders: 12,
            ..CorpusSpec::small()
        }
    }

    #[test]
    fn the_same_spec_generates_the_same_bytes() {
        let a: Vec<GeneratedMessage> = Corpus::new(tiny()).messages().collect();
        let b: Vec<GeneratedMessage> = Corpus::new(tiny()).messages().collect();
        assert_eq!(a, b);
        let other: Vec<GeneratedMessage> = Corpus::new(CorpusSpec { seed: 7, ..tiny() })
            .messages()
            .collect();
        assert_ne!(a[0].raw, other[0].raw);
    }

    #[test]
    fn folder_layout_has_the_requested_folders_and_weights() {
        for count in [1usize, 4, 8, 12, 40, 60] {
            let layout = folder_layout(count);
            assert_eq!(layout.len(), count, "{count}");
            let sum: f64 = layout.iter().map(|f| f.weight).sum();
            assert!((sum - 1.0).abs() < 1e-9, "{count}: {sum}");
            let mut names: Vec<&str> = layout.iter().map(|f| f.name.as_str()).collect();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), count, "unique names");
            // Parents come before their children.
            for (i, f) in layout.iter().enumerate() {
                if let Some((parent, _)) = f.name.split_once('/')
                    && PARENTS.contains(&parent)
                {
                    let p = layout.iter().position(|x| x.name == parent);
                    assert!(p.is_some_and(|p| p < i), "{} before its parent", f.name);
                }
            }
        }
        let standard = folder_layout(40);
        assert_eq!(standard[0].name, "INBOX");
        assert_eq!(standard[0].role, Role::Inbox);
        assert!(standard[0].weight > standard[5].weight);
    }

    #[test]
    fn messages_have_the_requested_shape() {
        let corpus = Corpus::new(CorpusSpec {
            messages: 3_000,
            ..CorpusSpec::small()
        });
        let stats = corpus.stats();
        assert_eq!(stats.messages, 3_000);
        let ratio = |n: u64| f64_of(n) / 3_000.0;
        assert!(
            (ratio(stats.with_attachment) - 0.1).abs() < 0.03,
            "{stats:?}"
        );
        assert!((ratio(stats.replies) - 0.4).abs() < 0.06, "{stats:?}");
        assert!(stats.per_folder[0] > stats.per_folder[1]);
        assert!(stats.per_folder.iter().all(|n| *n > 0), "{stats:?}");
        assert!(stats.unread > 0 && stats.unread < 1_500);
        // Mean size: bodies around 4 kB plus attachments around 60 kB on a
        // tenth of the messages.
        let mean = stats.bytes / stats.messages;
        assert!((3_000..30_000).contains(&mean), "mean {mean}");
    }

    #[test]
    fn messages_parse_and_thread() {
        let corpus = Corpus::new(tiny());
        let mut rare = 0;
        let mut ids = std::collections::HashSet::new();
        let mut replies = 0;
        for m in corpus.messages() {
            let parsed = mail_parser::MessageParser::default()
                .parse(&m.raw)
                .expect("parses");
            let id = parsed.message_id().expect("message id").to_owned();
            assert!(ids.insert(id));
            assert!(parsed.subject().is_some_and(|s| !s.is_empty()));
            assert!(parsed.date().is_some());
            assert!(m.raw.windows(2).all(|w| w != b"\n\n"), "CRLF only");
            if m.is_reply {
                replies += 1;
                let parent = parsed.in_reply_to().as_text().expect("in-reply-to");
                assert!(ids.contains(parent), "replies answer earlier messages");
                assert!(parsed.subject().unwrap_or_default().starts_with("Re: "));
            }
            if m.has_attachment {
                assert!(parsed.attachment_count() >= 1, "{}", m.index);
            }
            if String::from_utf8_lossy(&m.raw).contains(RARE_WORD) {
                rare += 1;
            }
            assert_ne!(m.internal_date().len(), 0);
        }
        assert_eq!(rare, 1);
        assert!(replies > 0);
    }

    #[test]
    fn arrivals_are_new_inbox_messages_after_the_corpus() {
        let corpus = Corpus::new(tiny());
        let last = corpus.messages().last().unwrap();
        let batch: Vec<GeneratedMessage> = corpus.arrivals(0, 20).collect();
        assert_eq!(batch.len(), 20);
        assert!(batch.iter().all(|m| m.folder == 0 && m.date > last.date));
        assert!(batch.iter().all(|m| !m.flags.contains(&"\\Seen")));
        let next: Vec<GeneratedMessage> = corpus.arrivals(20, 5).collect();
        assert_ne!(batch[0].raw, next[0].raw);
        assert_eq!(corpus.arrivals(0, 20).collect::<Vec<_>>(), batch);
        assert!(next[0].index > batch[19].index);
    }

    #[test]
    fn search_queries_name_real_senders() {
        let corpus = Corpus::new(tiny());
        let queries = corpus.search_queries();
        assert!(queries.len() >= 10);
        let name = corpus.people()[0].name.split_whitespace().last().unwrap();
        assert!(queries[0].contains(&name.to_ascii_lowercase()));
    }
}
