//! Small shared helpers without dependencies (wave Q2, SSOT / ODR / DRY,
//! ADR 0171): code that several crates had copied, defined once.
//!
//! - [`markup`]: HTML escaping.
//! - [`time`]: proleptic Gregorian calendar arithmetic (Howard Hinnant's
//!   civil-date algorithms), the current Unix time, RFC 3339 / ISO dates.
//!
//! A helper belongs here when crates on both sides of the dependency graph
//! (the engines and the core) need it and it needs no dependency; anything
//! with network types goes to `nail-net`, contract types stay in `nail-api`.
//! Q2 packages add their own module files (one `pub mod` line each in this
//! file).

pub mod markup;
pub mod time;
