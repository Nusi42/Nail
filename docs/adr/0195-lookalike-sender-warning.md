# 0195 — Warn about disguised senders

Status: accepted (2026-10-16)

## Context

The maintainer received a phishing mail that imitated their mail provider. Its
`From` was `"Kundenservice <BRAND in full-width letters>" <noreply@…>`, and
the domain was an internationalized one whose Unicode form looks like the
provider's domain: a Latin letter with a comma below in the name and a
dotted `ċ` in the top-level domain (`xn--…-6la`, "ċom"). SPF, DKIM and DMARC
reported `none`. The reader warned only about links and attachments
(ADR 0029); the link check also would not have flagged this domain: every
label is a single script, and the confusable skeleton of the accented
letters is not ASCII.

## Decision

`MessageWarning::LookalikeSender { address, unicode_domain, resembles, name }`
(`nail_mime::sender::sender_warnings`, called first by
`links::message_warnings`, so it is listed first in the reader's banner).

- **Domain.** Flagged when the link check's `lookalike` flags it as a host
  (mixed scripts, confusables, invalid punycode), or when its top-level domain
  is internationalized but folds to ASCII without its accents (`ċom` → `com`):
  no real top-level domain is Latin with accents. `resembles` is the folded
  domain (`ńovabank.ċom` → `novabank.com`). Ordinary IDNs (`münchen.de`,
  Cyrillic `.рф`) pass. An address in Unicode (SMTPUTF8) is converted with
  IDNA first; one that is no valid domain is flagged.
- **Display name.** Flagged when it contains invisible characters
  (zero-width, joiners, bidi controls, BOM, fillers) or letters and digits
  from the look-alike blocks — full-width ASCII, mathematical alphanumerics,
  circled / squared letters — unless the name also contains Chinese,
  Japanese or Korean characters (full-width Latin is ordinary there).
  Compatibility forms in general (`№`, `ﬁ`, superscripts) are not flagged.
  `name` is how it reads: those letters folded with NFKC, invisible
  characters removed.
- **UI.** The banner item (`data-kind="lookalike-sender"`) states the real
  address and what it imitates, how the name reads, and "Do not trust who
  this message claims to be from." Texts in `security.ftl`.
- New workspace dependency `unicode-normalization` (MIT / Apache-2.0),
  already in the lock through `unicode-security`.

## Consequences

- Accented Latin letters in the second-level label alone (a domain like
  `ștrato.com` with a real `.com`) are not flagged: legitimate IDNs look the
  same. The display-name and top-level-domain rules catch the common
  phishing forms.
- No brand list: `resembles` comes from folding only, so impersonation of a
  plain ASCII brand from an unrelated plain domain is left to the sender
  check (SPF / DKIM / DMARC badge).
