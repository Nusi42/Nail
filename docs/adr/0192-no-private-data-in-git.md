# ADR 0192: No private data in git — guard, generic paths, history rewrite

Status: accepted · 2026-10-09 · maintainer rule

## Context

An agent prompt carried details of the maintainer's real mail account (address, domain, provider host names, a folder
name) and the agent copied them into the connection-log tests and ADRs 0190 / 0191. The repository also named the
maintainer's Windows profile folder in many paths (`CLAUDE.md`, `docs/`, `.claude/memory/`, `.claude/workflows/`,
`scripts/`). Rule: none of the maintainer's private information may ever end up in
git — code, tests, docs, ADRs, memory, workflow arguments or commit messages. The Windows user name counts as
private. The git author address stays as it is.

## Decision

1. **Forward fix.** The leaked test data is replaced by neutral example data (`example.net`, the demo personas);
   every Windows path that named the profile folder reads `%USERPROFILE%\…` in prose and Windows docs and `~/…` in
   Git Bash commands. `.claude/workflows/nail-wave.js` cannot compute the home folder (the Workflow runtime has no
   Node APIs: no `process`, no `os`), so its prompts name folders as `<work root>\nail-work-<N>` in
   prose and `<work root>/nail-work-<N>` / `<master>` in commands (agents run Git Bash, which
   expands `~`).
2. **Guard** `scripts/check-private.mjs` (logic in `scripts/lib/private-patterns.mjs`):
   - The patterns live in a file outside every repository, named by `NAIL_PRIVATE_PATTERNS` (the environment, the
     checkout's gitignored `.env.local` or its master's; one regular expression per line, `#` comments, matched
     case-insensitively). There is no built-in location (ADR 0234). Unset, or the file absent, the guard prints a
     one-line "no private patterns configured" notice and passes (other machines).
     The file and its values are never copied into a repository, ADR, memory file or commit message.
   - For every pattern that is a plain literal (word boundaries and anchors aside), the guard also searches its
     base64 forms at the three byte alignments (the characters that depend only on the literal's bytes, standard
     and URL-safe alphabets; forms under five characters are skipped as too likely in random hashes), so encoded
     test data such as `AUTH PLAIN` tokens is caught. Encoded forms match exactly (base64 is case-sensitive).
   - Error lines name file, line and column and the pattern's line number; the matched text is masked (first
     character + `*`), file names in the output too.
   - Where it runs: `--staged` (pre-commit hook: the staged blobs, not the working tree, plus file names),
     `--message` (new commit-msg hook), the whole tracked tree (step `private` of `npm run check`), `--push`
     (pre-push hook: the messages, file names and added lines of the commits not yet on a remote; removed lines
     pass, so a commit that deletes private text can be pushed).
   - Tests use synthetic patterns only (`scripts/test/private-patterns.test.mjs`).
3. **History rewrite** (one-time). The history was rewritten once with `git filter-repo` (`--replace-text`,
   `--replace-message`): private details became neutral example data (`example.net`, "the provider", `<user>`),
   plus rules for their base64 forms. Verified clean: no pattern (or base64 form) in `git log --all -p`, in any
   commit message or in any working tree; author addresses unchanged.

## Consequences

- Old commit ids are gone; every clone of the old history must be discarded (there were no others).
- Findings from the maintainer's real accounts are passed to agents generically ("a real IMAP account", "a folder")
  with "do not copy any of these details into the repo" (memory `no-private-data-in-git`). Reviews treat private
  data in a diff as blocking.
- A new private detail is added by appending a pattern to the configured patterns file (never to the repository); the next
  `npm run check` then scans the whole tree for it.
