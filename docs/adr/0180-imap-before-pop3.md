# ADR 0180: IMAP before POP3 in account setup

Status: accepted · 2026-10-08 · maintainer finding (desktop test of 4290474)

## Context

Finding: account setup should default to IMAP, not POP3. Manual setup already starts with IMAP
(`manualCandidate`; the protocol select lists IMAP first), but discovery ranked candidates by encryption and
source only and kept each source's own order. Many providers' autoconfig / ISPDB files list POP3 before IMAP,
so the first proposal — the one setup preselects — was often POP3; an autoconfig POP3 also outranked an IMAP
proposal from a less trusted source. The candidate list did not say which protocol a proposal uses.

## Decisions

- `nail_discovery::rank` sorts by: plain-HTTP answers last, encrypted before unencrypted, **IMAP (and the API
  protocols) before POP3**, then source trust, then each source's own order. Encryption still wins over the
  protocol (an encrypted POP3 proposal comes before a plain-text IMAP one); the warning rules
  (`preferredCandidate` never preselects an unverified proposal) are unchanged.
- The candidate line names the protocol: "Incoming (IMAP) host:port, outgoing host:port"
  (`setup-candidate-servers` gets `$protocol`).
- `demo-multi`'s `new.example` autoconfig lists POP3 first, like those providers, so the core test
  (`engines.rs`) and the e2e scenario (`accounts.spec.ts`) prove that IMAP is proposed and preselected.

## Consequences

Setup proposes IMAP whenever a provider offers it; POP3 stays one click away (its radio, or the protocol
select under "Edit server settings").
