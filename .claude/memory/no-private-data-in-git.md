---
name: no-private-data-in-git
description: "Rule: none of the maintainer's private data may ever end up in git — not in code, tests, docs, ADRs, memory, commit messages or agent-written files"
metadata:
  node_type: memory
  type: feedback
  modified: 2026-10-08T23:52:01.347Z
---

Never commit the owner's private data: email addresses, domains, mail providers and server names of their accounts,
folder names, message counts / subjects from their mailbox, names, account ids, passwords, tokens, keychain contents,
anything read from their real profile (`%APPDATA%\Nail`, `%LOCALAPPDATA%\Nail`) or installers' logs. Test data and
examples use example.com / example.net / example.org and the demo personas (Sam Taylor, Ada Lovelace, …).
Also never: the owner's real name, real email addresses (also as commit author / committer), or the names of the
owner's other projects. Commits use the public GitHub noreply identity set in each folder's local
git config (ADR 0198).

**Why:** mandatory maintainer rule. It was broken once: an agent prompt carried the owner's real account
details (address, provider, folder names) and the agent put them into nail-connlog tests and ADRs 0190 / 0191; fixed
forward.
**How to apply:** when passing a bug report from the owner's real account to an agent, describe it generically ("a real
IMAP account", "a folder", counts only if needed) and add "do not copy any of these details into the repo". Reviews
treat private data in a diff as a blocking finding. The automatic guard (patterns kept outside the repo in
`%USERPROFILE%\.nail-private\patterns.txt`, checked by `scripts/check-private.mjs` in the pre-commit / commit-msg / pre-push hooks and `npm run check`; ADR 0192) must stay on. History was rewritten twice: ADR 0192 and ADR 0198 (identities, the real name in a byline,
another project's name). The guard also checks commit identities. The owner's GitHub handle is in
the patterns but allowed in its GitHub forms (noreply identity, github.com URLs) only.

Related: [[ssot-odr-dry]], [[zero-warnings-policy]]
