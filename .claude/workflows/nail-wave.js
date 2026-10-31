export const meta = {
  name: 'nail-wave',
  description: 'Run one Nail implementation wave (contract, 4 parallel packages, integrate, 2 reviews, fix, verify) from args; Opus 5.5 medium agents',
  phases: [
    { title: 'Contract' },
    { title: 'Build' },
    { title: 'Integrate' },
    { title: 'Review' },
    { title: 'Fix' },
    { title: 'Verify' },
  ],
}

const A = args
// Master repository and work folders (docs/dev-setup.md "Work folders",
// ADRs 0132, 0199): the master checkout only receives pushes and
// fast-forward merges. The lead stages (contract, integrate, reviews, fix,
// verify) work in work folder <lead>; after each green lead stage main is
// fast-forwarded to the lead's branch (FF below), so main is always green.
// Parallel package i resets work folder <lead+1+i> (or b.folder) to main,
// works on its branch and pushes it.
// Where they are is the maintainer's setup, never in the repository: the
// args name it (the Workflow runtime has no Node APIs to read .env.local).
//   master / masterSh: the master in prose (Windows) and Git Bash form
//   workRoot / workRootSh: the folder that holds the work folders
//   folderPrefix / branchPrefix: default nail-work- / work- (as scripts/lib/work-folder.mjs)
//   trailer: the Co-Authored-By line (never a session line, ADR 0200)
for (const key of ['master', 'masterSh', 'workRoot', 'workRootSh', 'trailer']) {
  if (!A[key]) throw new Error(`nail-wave: args.${key} is required (see the comment above)`)
}
const MASTER = A.master
const MASTER_SH = A.masterSh
const LEAD = A.lead || 1
const FOLDER_PREFIX = A.folderPrefix || 'nail-work-'
const folder = n => `${A.workRoot}\\${FOLDER_PREFIX}${n}`
const folderSh = n => `${A.workRootSh}/${FOLDER_PREFIX}${n}`
const REPO = folder(LEAD)
const REPO_SH = folderSh(LEAD)
const branchOf = n => `${A.branchPrefix || 'work-'}${n}`
const LEAD_BRANCH = branchOf(LEAD)
const slotOf = (b, i) => b.folder || LEAD + 1 + i
// The last step of every green lead stage that commits.
const FF = `then fast-forward the master: \`git push origin ${LEAD_BRANCH} && git -C ${MASTER_SH} merge --ff-only ${LEAD_BRANCH}\` (the only write ever made in the master; if it fails, report it and stop — never edit files there)`
const OPTS = { model: 'opus', effort: 'medium' }
const TRAILER = A.trailer

const COMMON = `You are implementing "Nail", a Rust + TypeScript/React email & PIM client (Windows 11; Bash tool = Git Bash with POSIX paths like ${REPO_SH}; %USERPROFILE% in this prompt is your home folder, ~ in Git Bash; PowerShell also available). Your shell may start in another directory: cd into your work folder in every command or use absolute paths.
WHERE YOU WORK: ${MASTER} is the MASTER repository. Never edit, build, test, run or commit anything there (its npm scripts, cargo and git hook refuse anyway). Your work folder is named in your task; the lead's is ${REPO} (branch ${LEAD_BRANCH}). Build output stays inside your work folder (target/, .nail-build/).
Earlier milestones are done (git log; .claude/memory/*status*.md). We are now in wave ${A.wave}. Read docs/plan.md ${A.planRef} fully (including its "Scope decisions"), the ADRs in docs/adr/ relevant to your area, docs/dev-setup.md, and the relevant sections of docs/design.md. CLAUDE.md (in your context; re-read your work folder's copy, it may be newer) lists commands and conventions — follow them.
Mandatory rules:
- Tests from day 1: every change ships with meaningful unit tests (and Playwright scenarios where your package owns e2e). Never report success with failing or skipped tests; if something cannot pass, say exactly what and why.
- Internal docs -> docs/ (ADRs docs/adr/NNNN-title.md; in parallel stages suffix with your package key, e.g. 0029-wp31-pgp.md; the integrator renumbers). User-facing docs -> userDocs/en/ (userDocs/de/ stays empty).
- No literal user-visible strings in UI or user-facing Rust output; Fluent catalogs locales/en/*.ftl complete, locales/de/*.ftl same file names but empty; per-key English fallback.
- React components never use inline styles or literal classNames; data-part attributes only; styling only from the compiled theme; structure only from layout.kdl.
- The core owns data; UI talks only through NailClient; every write goes through the operation queue.
- Licences: Nail is AGPL-3.0-or-later and dual-licensed (ADR 0198) — do not add GPL/LGPL/AGPL dependencies (MPL-2.0, MIT, Apache-2.0, BSD, ISC, Zlib, Unicode are fine). Check licences before adding a crate/package. Avoid an OpenSSL build dependency unless an ADR justifies it.
- The machine is shared with other development work (other Vite / tauri dev projects may be running and holding ports and memory): run cargo with -j 4 (os error 1455 = out of page file -> rerun with -j 2), never bind fixed development ports (take free ports), and never kill processes you did not start.
- No network except localhost mocks and package registries. A Vite dev server / 'tauri dev' may be running from the user's session: never kill it; if npm ci hits a locked file use npm install and say so.
- Prefer well-maintained crates/packages; verify real APIs in ~/.cargo/registry or node_modules rather than guessing.
- SSOT / ODR / DRY (mandatory, CLAUDE.md): every fact (type, constant, id, limit, size, string, rule, mapping, schema) has exactly one source of truth and one definition; other places derive it (generated code, imports, shared tokens, shared helpers) instead of mirroring or copying it by hand. No copy-pasted logic: extract a shared function / component / test helper when the second copy appears. Rust <-> TS facts are generated (ts-rs / gen:api) or checked by a test, never hand-mirrored. The duplicate check in npm run check must stay at 0 findings.`

const RESULT = {
  type: 'object',
  properties: {
    summary: { type: 'string' },
    filesChanged: { type: 'array', items: { type: 'string' } },
    testCommands: { type: 'array', items: { type: 'string' } },
    allTestsPass: { type: 'boolean' },
    testReport: { type: 'string' },
    apiRequests: { type: 'array', items: { type: 'string' } },
    followUps: { type: 'array', items: { type: 'string' } },
  },
  required: ['summary', 'filesChanged', 'testCommands', 'allTestsPass', 'testReport', 'apiRequests', 'followUps'],
}

phase('Contract')
const contract = await agent(`${COMMON}

YOUR WORK FOLDER: ${REPO} (branch ${LEAD_BRANCH}). Start with \`git fetch origin\` and fast-forward to origin/${LEAD_BRANCH} if it moved; the tree must be clean (else stop and report).
YOUR TASK: ${A.contract}
You are the only agent running now; four agents start after you in parallel, each in its own work folder, and build against what you create. Besides the contract, prepare collision-free slots for them: stub crates (compiling, trivial passing test, added to the workspace, likely deps declared in [workspace.dependencies] and verified to build on Windows MSVC with acceptable licences), UI feature folders ui/src/features/<name>/index.ts registered centrally, locale files locales/en/<name>.ftl + empty locales/de/<name>.ftl, empty modules in shared crates where a parallel package needs one. Parallel package slots needed: ${A.slots}.
Write an ADR for the contract. Update docs/architecture.md (code map, milestone notes) and docs/dev-setup.md (commands) if they change; CLAUDE.md only for new rules. Run the full suite (npm run build, cargo fmt --check, cargo clippy --workspace --all-targets -- -D warnings, cargo test --workspace, npm run lint, npm test, npm run e2e:only). Commit with a Conventional Commits message that says in plain English what changed (CLAUDE.md "Git"; the commit-msg hook checks it) ending with the trailer below; ${FF}. The parallel packages start from main:
${TRAILER}`, { ...OPTS, label: `${A.wave} contract`, phase: 'Contract', schema: RESULT })

// args.acceptContract: resume past a contract stage whose only red test was
// a known pre-existing flake (set by the orchestrator after checking).
if (!contract || (!contract.allTestsPass && !A.acceptContract)) {
  log('Contract stage not green; stopping.')
  return { stoppedAt: 'Contract', contract }
}

const RULES = (key, dirs, npm, n) => `
YOUR WORK FOLDER: ${folder(n)} (branch ${branchOf(n)}). Four agents work at the same time, each in its own work folder.
- Start: cd there; the tree must be clean (\`git status --porcelain\` empty, else stop and report); \`git fetch origin && git switch ${branchOf(n)} && git reset --hard origin/main\` (= the contract); \`npm ci\` (npm install if files are locked; say so).
- Finish: commit your work on ${branchOf(n)} (a Conventional Commits message that says in plain English what changed, CLAUDE.md "Git"; trailer below; the pre-commit hook must pass, never --no-verify), then \`git push --force-with-lease origin ${branchOf(n)}\`. The integrator rebases ${branchOf(n)} onto ${LEAD_BRANCH}. No other git writes.
${TRAILER}
- You own ONLY: ${dirs}. Do not edit anything else (not crates/nail-api, not crates/nail-core, not shared registries, defaults/*.kdl, nail-api fixtures, root Cargo.toml/package.json, or other agents' files). Register UI only through your feature's index.ts; strings only in your own locale file. Everything else goes into apiRequests with exact details for the integrator.
- Dependencies only in your own crates' Cargo.toml (workspace = true where declared). ${npm ? 'You MAY add npm packages to ui with npm install -w ui <pkg>.' : 'Do NOT change package.json/package-lock.json.'}
- Use -p <crate> with cargo (default target dir of your work folder).`

phase('Build')
const built = await parallel(A.build.map((b, i) => () =>
  agent(`${COMMON}
${RULES(b.key, b.dirs, b.npm, slotOf(b, i))}

Contract agent's report:
${contract.summary}
Contract follow-ups: ${(contract.followUps || []).join(' | ')}

YOUR TASK: ${b.prompt}`, { ...OPTS, label: b.key, phase: 'Build', schema: RESULT })
    .then(r => ({ key: b.key, ...(r || { summary: 'AGENT FAILED', allTestsPass: false, apiRequests: [], followUps: [], filesChanged: [], testCommands: [], testReport: '' }) }))
))
const buildReport = built.map((r, i) => `### ${r.key} (branch ${branchOf(slotOf(A.build[i], i))}) — allTestsPass=${r.allTestsPass}
${r.summary}
Tests: ${r.testReport}
API requests: ${(r.apiRequests || []).join(' | ') || 'none'}
Follow-ups: ${(r.followUps || []).join(' | ') || 'none'}`).join('\n\n')
log(`Build: ${built.filter(r => r.allTestsPass).length}/${built.length} green`)

phase('Integrate')
const integ = await agent(`${COMMON}

YOUR TASK: integration of ${A.wave} in YOUR WORK FOLDER ${REPO} (branch ${LEAD_BRANCH}). You are the only agent now; you may edit anything. Use the default cargo target dir.
Reports from the parallel packages (each committed on its branch and pushed to origin):
${buildReport}

0. \`git fetch origin\`, then rebase every package branch onto ${LEAD_BRANCH}, one after the other, never with a merge commit (ADR 0208; the pre-push hook refuses them): \`git switch -C integrate origin/<branch> && git rebase ${LEAD_BRANCH} && git switch ${LEAD_BRANCH} && git merge --ff-only integrate\`, resolving conflicts per commit. The branches: ${A.build.map((b, i) => `origin/${branchOf(slotOf(b, i))} (${b.key})`).join(', ')}. A branch that was not pushed means that package failed: say so and finish its work yourself.
1. Get everything green first (npm run build, cargo fmt/clippy -D warnings/test --workspace, npm run lint, npm test, npm run e2e:only). Resolve all apiRequests, renumber ADRs consistently, regenerate TS types. Commit with a Conventional Commits message that says in plain English what changed (CLAUDE.md "Git"; the commit-msg hook checks it) (trailer below).
2. ${A.integrate}
3. Update userDocs/en, docs/dev-setup.md, docs/architecture.md (code map, milestone notes), CLAUDE.md (rules only), and docs/plan.md (status line for this wave, follow-ups/deferrals). Add a memory file .claude/memory/${A.memoryName}.md (frontmatter name/description/metadata.type project; what was integrated, pitfalls) and a line in .claude/memory/MEMORY.md.
4. Full run incl. npm run e2e. Commit with a Conventional Commits message that says in plain English what changed (CLAUDE.md "Git"; the commit-msg hook checks it) with the trailer below; ${FF}:
${TRAILER}`, { ...OPTS, label: `${A.wave} integrate`, phase: 'Integrate', schema: RESULT })

const FINDINGS = {
  type: 'object',
  properties: {
    findings: { type: 'array', items: { type: 'object', properties: {
      severity: { type: 'string', enum: ['critical', 'high', 'medium', 'low'] },
      file: { type: 'string' }, line: { type: 'number' }, title: { type: 'string' }, detail: { type: 'string' },
    }, required: ['severity', 'file', 'title', 'detail'] } },
    overall: { type: 'string' },
  },
  required: ['findings', 'overall'],
}
phase('Review')
const reviews = await parallel(A.reviews.map(r => () =>
  agent(`${COMMON}

YOUR TASK: review ${A.wave} in ${REPO} (read-only: do not modify tracked files, do not commit). Lens: ${r.lens}
Integration report: ${integ ? integ.summary + '\nTests: ' + integ.testReport : 'integration agent failed'}
Use CARGO_TARGET_DIR=${REPO_SH}/.nail-build/target-review-${r.key}. Report only real, verified problems with concrete evidence; no nits. Rank by severity.
Always also check the wave's changes (git diff ${A.since}..HEAD) for SSOT / ODR / DRY violations: a fact defined or hand-mirrored in more than one place, copy-pasted logic, a second definition of an existing type / constant / helper. Report each as severity "medium" or higher (must-fix), naming both places and the single source it should come from.`, { ...OPTS, label: `review:${r.key}`, phase: 'Review', schema: FINDINGS })
))
const all = reviews.filter(Boolean).flatMap(r => r.findings)
const must = all.filter(f => f.severity !== 'low')
const lows = all.filter(f => f.severity === 'low')
log(`Review: ${must.length} critical/high/medium, ${lows.length} low`)

phase('Fix')
const fix = await agent(`${COMMON}

YOUR TASK: fix the review findings for ${A.wave} in ${REPO} (branch ${LEAD_BRANCH}). For each: confirm it is real (else explain and skip), fix it, add a regression test. Run the full suite (npm run build, cargo fmt --check, clippy -D warnings, cargo test --workspace, npm run lint, npm test — twice, to catch flaky tests — and npm run e2e). Update docs/plan.md status and the wave's memory file if findings change them. Commit with a Conventional Commits message that says in plain English what changed (CLAUDE.md "Git"; the commit-msg hook checks it) with the trailer below; ${FF}:
${TRAILER}

MUST-FIX:
${JSON.stringify(must, null, 1)}

OPTIONAL (low):
${JSON.stringify(lows, null, 1)}`, { ...OPTS, label: 'fix findings', phase: 'Fix', schema: RESULT })

phase('Verify')
const GATE = {
  type: 'object',
  properties: {
    gatePassed: { type: 'boolean' },
    checks: { type: 'array', items: { type: 'object', properties: { check: { type: 'string' }, passed: { type: 'boolean' }, evidence: { type: 'string' } }, required: ['check', 'passed', 'evidence'] } },
    remainingIssues: { type: 'array', items: { type: 'string' } },
  },
  required: ['gatePassed', 'checks', 'remainingIssues'],
}
const verify = await agent(`${COMMON}

YOUR TASK: independent final verification of ${A.wave} in ${REPO} (read-only; do not modify tracked files; do not commit). Use CARGO_TARGET_DIR=${REPO_SH}/.nail-build/target-verify-${A.key}.
Run from scratch: npm ci (or npm install if a dev server locks files — say so); npm run build; cargo fmt --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace; npm run lint; npm test (run it twice); npm run e2e; cargo build -p nail-app.
Then: ${A.verify}
Also check docs (architecture.md, dev-setup.md), CLAUDE.md, plan and memory updated, git status clean, ${LEAD_BRANCH} pushed and the master's main fast-forwarded to it (origin/${LEAD_BRANCH} = origin/main = HEAD), and list commits since ${A.since}. Evidence for each check.`, { ...OPTS, label: `verify ${A.wave}`, phase: 'Verify', schema: GATE })

return {
  wave: A.wave,
  contract: { allTestsPass: contract.allTestsPass, followUps: contract.followUps },
  build: built.map(r => ({ key: r.key, allTestsPass: r.allTestsPass, followUps: r.followUps })),
  integrate: integ && { allTestsPass: integ.allTestsPass, testReport: integ.testReport, followUps: integ.followUps },
  review: { must: must.length, low: lows.length, titles: all.map(f => `${f.severity}: ${f.title}`) },
  fix: fix && { allTestsPass: fix.allTestsPass, testReport: fix.testReport, followUps: fix.followUps },
  verify,
}
