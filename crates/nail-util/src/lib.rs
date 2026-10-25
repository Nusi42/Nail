//! Small shared helpers without dependencies (wave Q2, SSOT / ODR / DRY,
//! ADR 0171): code that several crates had copied, defined once.
//!
//! - [`batch`]: batched writes with a one-by-one fallback.
//! - [`color`]: sRGB hex colours, WCAG luminance and contrast ratios.
//! - [`expiring`]: a thread-safe map whose entries expire (unlock caches).
//! - [`group`]: grouping in first-seen order.
//! - [`ical_text`]: iCalendar UTC stamps and durations (RFC 5545 texts).
//! - [`markup`]: HTML escaping.
//! - [`mime_text`]: byte search, header unfolding, backslash escapes,
//!   `Bcc` headers of raw messages.
//! - [`named`]: case-insensitive lookups in (name, value) pair lists.
//! - [`percent`]: percent-decoding of URL parts.
//! - [`redact`]: the stand-in text of a secret.
//! - [`sieve_lex`]: the lexer of Sieve scripts.
//! - [`snapshot`]: snapshot and golden files of tests (write or compare).
//! - [`time`]: proleptic Gregorian calendar arithmetic (Howard Hinnant's
//!   civil-date algorithms), the current Unix time, RFC 3339 / ISO dates
//!   (seconds and milliseconds, now and of a given time).
//!
//! A helper belongs here when crates on both sides of the dependency graph
//! (the engines and the core) need it and it needs no dependency; anything
//! with network types goes to `nail-net`, contract types stay in `nail-api`.
//! Q2 packages add their own module files (one `pub mod` line each in this
//! file).

pub mod batch;
pub mod color;
pub mod expiring;
pub mod group;
pub mod ical_text;
pub mod markup;
pub mod mime_text;
pub mod named;
pub mod percent;
pub mod redact;
pub mod sieve_lex;
pub mod snapshot;
pub mod time;
