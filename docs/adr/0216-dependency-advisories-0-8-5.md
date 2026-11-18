# 0216 — Dependency advisories before 0.8.5

Status: accepted (2026-11-18)

## Context

`release:prepare` runs `cargo deny` with advisories (ADR 0209). Before
0.8.5 it reported twelve RustSec advisories that appeared after 0.8.4:

- `ammonia` 4.2.4: mutation XSS through embedded SVG and MathML
  (RUSTSEC-2026-0533). ammonia sanitizes HTML mail and composer HTML.
- `lettre` 0.11.23: seven SMTP advisories (RUSTSEC-2026-0560 to 0566),
  among them STARTTLS reply injection, command injection through missing
  dot-stuffing and mail to a different address for a quoted local part.
- `rmcp` 3.5.0: three advisories in the OAuth client and the Streamable HTTP
  transports (RUSTSEC-2026-0457, 0461, 0462).
- `chrono` 0.4 and `chrono-tz` 0.10: unmaintained (RUSTSEC-2026-0543,
  RUSTSEC-2026-0555). No known flaw, and no fixed release.

`async-executor` 1.14.0 was also yanked.

## Decision

- Update `ammonia` to 4.2.5, `lettre` to 0.11.24, `rmcp` to 3.11.0 and
  `async-executor` to 1.14.1. The workspace's minimum versions follow, so an
  older lock cannot come back. `nail-mime` takes ammonia from the workspace
  instead of its own version line.
- Accept `chrono` and `chrono-tz` as unmaintained: both are ignored in
  `deny.toml` with a reason. Twenty Nail crates use chrono, and so do
  calcard, oauth2, rmcp and schemars. Replacing it in Nail alone would keep
  chrono in the build through those crates.

## Consequences

- 0.8.5 ships the fixed sanitizer, SMTP client and MCP library; its release
  notes have a security section.
- The chrono entries are removed from `deny.toml` once calcard, oauth2, rmcp
  and schemars have moved to `jiff` or `time` and Nail has followed. Each
  release's advisory check shows when that becomes possible.
