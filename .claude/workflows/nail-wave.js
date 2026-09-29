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
const REPO = '<master>'
const OPTS = { model: 'opus', effort: 'medium' }
const TRAILER = `Co-Authored-By: Claude Opus 5.5 (1M context) <noreply@anthropic.com>
Claude-Session: a published page, not linked here`

const COMMON = `You are implementing "Nail", a Rust + TypeScript/React email & PIM client, in the repo at ${REPO} (Windows 11; Bash tool = Git Bash with POSIX paths like /c/Users/<user>/OneDrive/<projects>/nail; PowerShell also available).
M0 and M1 are done (git log; .claude/memory/*status*.md). We are now in wave ${A.wave}. Read docs/plan.md ${A.planRef} fully (including its "Scope decisions"), the ADRs in docs/adr/ relevant to your area, docs/dev-setup.md, and the relevant sections of docs/design.md. CLAUDE.md (in your context) lists commands and conventions — follow them.
Mandatory rules:
- Tests from day 1: every change ships with meaningful unit tests (and Playwright scenarios where your package owns e2e). Never report success with failing or skipped tests; if something cannot pass, say exactly what and why.
- Internal docs -> docs/ (ADRs docs/adr/NNNN-title.md; in parallel stages suffix with your package key, e.g. 0029-wp31-pgp.md; the integrator renumbers). User-facing docs -> userDocs/en/ (userDocs/de/ stays empty).
- No literal user-visible strings in UI or user-facing Rust output; Fluent catalogs locales/en/*.ftl complete, locales/de/*.ftl same file names but empty; per-key English fallback.
- React components never use inline styles or literal classNames; data-part attributes only; styling only from the compiled theme; structure only from layout.kdl.
- The core owns data; UI talks only through NailClient; every write goes through the operation queue.
- Licences: Nail is closed source for now — do not add GPL/LGPL/AGPL dependencies (MPL-2.0, MIT, Apache-2.0, BSD, ISC, Zlib, Unicode are fine). Check licences before adding a crate/package. Avoid an OpenSSL build dependency unless an ADR justifies it.
- Never write build output into the repo (OneDrive). No network except localhost mocks and package registries. A Vite dev server / 'tauri dev' may be running from the user's session: never kill it; if npm ci hits a locked file use npm install and say so.
- Prefer well-maintained crates/packages; verify real APIs in ~/.cargo/registry or node_modules rather than guessing.`

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

YOUR TASK: ${A.contract}
You are the only agent running now; four agents start after you in parallel and build against what you create. Besides the contract, prepare collision-free slots for them: stub crates (compiling, trivial passing test, added to the workspace, likely deps declared in [workspace.dependencies] and verified to build on Windows MSVC with acceptable licences), UI feature folders ui/src/features/<name>/index.ts registered centrally, locale files locales/en/<name>.ftl + empty locales/de/<name>.ftl, empty modules in shared crates where a parallel package needs one. Parallel package slots needed: ${A.slots}.
Write an ADR for the contract. Update CLAUDE.md layout/commands if they change. Run the full suite (npm run build, cargo fmt --check, cargo clippy --workspace --all-targets -- -D warnings, cargo test --workspace, npm run lint, npm test, npm run e2e:only). Commit "${A.wave}: contract and slots" ending with:
${TRAILER}`, { ...OPTS, label: `${A.wave} contract`, phase: 'Contract', schema: RESULT })

if (!contract || !contract.allTestsPass) {
  log('Contract stage not green; stopping.')
  return { stoppedAt: 'Contract', contract }
}

const RULES = (key, dirs, npm) => `
PARALLEL WORK RULES (4 agents work at the same time in the same checkout):
- You own ONLY: ${dirs}. Do not edit anything else (not crates/nail-api, not crates/nail-core, not shared registries, defaults/*.kdl, nail-api fixtures, root Cargo.toml/package.json, or other agents' files). Register UI only through your feature's index.ts; strings only in your own locale file. Everything else goes into apiRequests with exact details for the integrator.
- Dependencies only in your own crates' Cargo.toml (workspace = true where declared). ${npm ? 'You MAY add npm packages to ui with npm install -w ui <pkg>.' : 'Do NOT change package.json/package-lock.json.'}
- Own cargo target dir: prefix cargo with CARGO_TARGET_DIR=/c/Users/<user>/AppData/Local/nail-build/target-${key} and use -p <crate>.
- Do NOT run git commands that write. The integrator commits.`

phase('Build')
const built = await parallel(A.build.map(b => () =>
  agent(`${COMMON}
${RULES(b.key, b.dirs, b.npm)}

Contract agent's report:
${contract.summary}
Contract follow-ups: ${(contract.followUps || []).join(' | ')}

YOUR TASK: ${b.prompt}`, { ...OPTS, label: b.key, phase: 'Build', schema: RESULT })
    .then(r => ({ key: b.key, ...(r || { summary: 'AGENT FAILED', allTestsPass: false, apiRequests: [], followUps: [], filesChanged: [], testCommands: [], testReport: '' }) }))
))
const buildReport = built.map(r => `### ${r.key} — allTestsPass=${r.allTestsPass}
${r.summary}
Tests: ${r.testReport}
API requests: ${(r.apiRequests || []).join(' | ') || 'none'}
Follow-ups: ${(r.followUps || []).join(' | ') || 'none'}`).join('\n\n')
log(`Build: ${built.filter(r => r.allTestsPass).length}/${built.length} green`)

phase('Integrate')
const integ = await agent(`${COMMON}

YOUR TASK: integration of ${A.wave}. You are the only agent now; you may edit anything. Use the default cargo target dir.
Reports from the parallel packages (uncommitted work in the tree):
${buildReport}

1. Get everything green first (npm run build, cargo fmt/clippy -D warnings/test --workspace, npm run lint, npm test, npm run e2e:only). Resolve all apiRequests, renumber ADRs consistently, regenerate TS types. Commit "${A.wave}: parallel packages" (trailer below).
2. ${A.integrate}
3. Update userDocs/en, docs/dev-setup.md, CLAUDE.md, and docs/plan.md (status line for this wave, follow-ups/deferrals). Add a memory file .claude/memory/${A.memoryName}.md (frontmatter name/description/metadata.type project; what was integrated, pitfalls) and a line in .claude/memory/MEMORY.md.
4. Full run incl. npm run e2e. Commit "${A.wave}: integration and e2e" with trailer:
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

YOUR TASK: review ${A.wave} (read-only: do not modify tracked files, do not commit). Lens: ${r.lens}
Integration report: ${integ ? integ.summary + '\nTests: ' + integ.testReport : 'integration agent failed'}
Use CARGO_TARGET_DIR=/c/Users/<user>/AppData/Local/nail-build/target-review-${r.key}. Report only real, verified problems with concrete evidence; no nits. Rank by severity.`, { ...OPTS, label: `review:${r.key}`, phase: 'Review', schema: FINDINGS })
))
const all = reviews.filter(Boolean).flatMap(r => r.findings)
const must = all.filter(f => f.severity !== 'low')
const lows = all.filter(f => f.severity === 'low')
log(`Review: ${must.length} critical/high/medium, ${lows.length} low`)

phase('Fix')
const fix = await agent(`${COMMON}

YOUR TASK: fix the review findings for ${A.wave}. For each: confirm it is real (else explain and skip), fix it, add a regression test. Run the full suite (npm run build, cargo fmt --check, clippy -D warnings, cargo test --workspace, npm run lint, npm test — twice, to catch flaky tests — and npm run e2e). Update docs/plan.md status and the wave's memory file if findings change them. Commit "${A.wave}: fix review findings" with trailer:
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

YOUR TASK: independent final verification of ${A.wave} (read-only; do not modify tracked files; do not commit). Use CARGO_TARGET_DIR=/c/Users/<user>/AppData/Local/nail-build/target-verify-${A.key}.
Run from scratch: npm ci (or npm install if a dev server locks files — say so); npm run build; cargo fmt --check; cargo clippy --workspace --all-targets -- -D warnings; cargo test --workspace; npm run lint; npm test (run it twice); npm run e2e; cargo build -p nail-app.
Then: ${A.verify}
Also check docs/CLAUDE.md/plan/memory updated, git status clean, list commits since ${A.since}. Evidence for each check.`, { ...OPTS, label: `verify ${A.wave}`, phase: 'Verify', schema: GATE })

return {
  wave: A.wave,
  contract: { allTestsPass: contract.allTestsPass, followUps: contract.followUps },
  build: built.map(r => ({ key: r.key, allTestsPass: r.allTestsPass, followUps: r.followUps })),
  integrate: integ && { allTestsPass: integ.allTestsPass, testReport: integ.testReport, followUps: integ.followUps },
  review: { must: must.length, low: lows.length, titles: all.map(f => `${f.severity}: ${f.title}`) },
  fix: fix && { allTestsPass: fix.allTestsPass, testReport: fix.testReport, followUps: fix.followUps },
  verify,
}
