/**
 * The mock core's answers to the M4c import / export / backup / restore /
 * diagnostics requests and the desktop file dialog (ADR 0094), used by
 * `MockTransport` (`../../api/mock.ts` routes exactly these requests here).
 *
 * It simulates what `nail-portability` and the core do (WP121 / WP125):
 * previews of the six import formats (Thunderbird with account and filter
 * proposals), jobs that report `PortabilityJobProgress` a few times and end
 * with `PortabilityJobFinished` and a report with per-item issues, export
 * conflicts, encrypted backups kept by path and passphrase (wrong
 * passphrase = `auth_failed` + `backup-wrong-passphrase`), the restore dry
 * run, and the diagnostics preview that lists exactly what the bundle
 * holds. `PickPath` answers like the desktop host when `desktop` is on
 * (from `pickAnswers`, else a plausible path), else `unsupported`
 * (`desktop-unavailable`) like `nail serve`.
 */
import type {
  AccountProposal,
  BackupContents,
  BackupManifest,
  DiagnosticsEntry,
  DiagnosticsManifest,
  DiagnosticsOptions,
  ExportFormat,
  ExportSource,
  ImportOptions,
  ImportPreview,
  ImportSource,
  ItemIssue,
  JobCounts,
  JobKind,
  JobPhase,
  JobReport,
  JobStatus,
  PathPick,
  PortabilityIssueCode,
  Request,
  Response,
  RestoreAction,
  RestorePlan,
  RuleProposal,
} from "../../api/generated";
import { mockAccountConfig } from "../../api/mockData";
import type { MockCore } from "../../api/mockMail";
import { sampleRule } from "../rules/mock";
import {
  BACKUP_ITEMS,
  backupContentsIssues,
  baseName,
  diagnosticsIssues,
  exportAccepts,
  isAbsolutePath,
  passphraseIssues,
} from "./model";

/** The requests this feature answers. */
export const PORTABILITY_REQUESTS = [
  "FindImportSources",
  "PreviewImport",
  "StartImport",
  "StartExport",
  "CreateBackup",
  "InspectBackup",
  "VerifyBackup",
  "PreviewRestore",
  "StartRestore",
  "ListPortabilityJobs",
  "GetPortabilityJob",
  "CancelPortabilityJob",
  "PreviewDiagnostics",
  "CreateDiagnosticsBundle",
  "PickPath",
] as const satisfies readonly Request["type"][];

export type PortabilityRequest = Extract<Request, { type: (typeof PORTABILITY_REQUESTS)[number] }>;

export function isPortabilityRequest(request: Request): request is PortabilityRequest {
  return (PORTABILITY_REQUESTS as readonly string[]).includes(request.type);
}

/** A backup the mock "wrote". */
export interface MockBackup {
  passphrase: string;
  manifest: BackupManifest;
  /** Entry names that are damaged (verify / restore report them). */
  damaged: string[];
}

/** A running job's plan: the phases it walks through and its final report. */
interface MockJob {
  status: JobStatus;
  phases: JobPhase[];
  step: number;
  total: number;
  report: JobReport;
}

/** Mock-core state of this feature. */
export interface PortabilityMockState {
  /** Whether a desktop host exists (file dialogs; `false` = `nail serve`). */
  desktop: boolean;
  /** Answers of the next file dialogs (`null` = the user cancelled). */
  pickAnswers: (string | null)[];
  /** Paths that do not exist (`not_found`). */
  missing: string[];
  /** Files that exist (export targets without `overwrite` → `conflict`). */
  existing: string[];
  /** Backups by path. */
  backups: Record<string, MockBackup>;
  /** Jobs by id. */
  jobs: Record<string, MockJob>;
  /** Steps run on their own (`false`: tests call `advancePortabilityJobs`). */
  autoProgress: boolean;
  /** Progress events per phase. */
  stepsPerPhase: number;
  /** Bundles written. */
  bundles: string[];
}

export function createPortabilityMockState(): PortabilityMockState {
  return {
    desktop: false,
    pickAnswers: [],
    missing: [],
    existing: [],
    backups: {},
    jobs: {},
    autoProgress: true,
    stepsPerPhase: 2,
    bundles: [],
  };
}

/** `unsupported` with a Fluent id, as the core answers. */
export function notReady(request: Request, id = "m4c-not-ready"): Response {
  return {
    type: "Error",
    code: "unsupported",
    message: `${request.type} is not available yet`,
    localized: { id },
  };
}

function fail(
  code: Extract<Response, { type: "Error" }>["code"],
  message: string,
  id?: string,
  args?: Record<string, string | number>,
): Response {
  return {
    type: "Error",
    code,
    message,
    ...(id ? { localized: { id, ...(args ? { args } : {}) } } : {}),
  };
}

export function emptyCounts(): JobCounts {
  return {
    messages: 0,
    folders: 0,
    events: 0,
    tasks: 0,
    contacts: 0,
    groups: 0,
    accounts: 0,
    rules: 0,
    files: 0,
    duplicates: 0,
    skipped: 0,
  };
}

function issue(
  code: PortabilityIssueCode,
  severity: ItemIssue["severity"],
  item: Partial<ItemIssue["item"]>,
  detail: string,
): ItemIssue {
  return {
    item: { file: null, folder: null, index: null, offset: null, label: null, ...item },
    severity,
    code,
    detail,
    localized: { id: `portability-issue-${code}` },
  };
}

// ---------------------------------------------------------------------------
// Import previews

function accountProposals(core: MockCore): AccountProposal[] {
  const existing = core.data.accountsConfig.accounts.find((a) => a.email === "ada@example.org");
  return [
    {
      key: "account1",
      config: {
        ...mockAccountConfig("", "Ada (Thunderbird)", "ada@example.org", "example.org", []),
      },
      duplicate_of: existing?.id ?? null,
      issues: [
        issue(
          "secret-not-imported",
          "warning",
          { label: "ada@example.org" },
          "the password stays in Thunderbird",
        ),
      ],
    },
    {
      key: "account2",
      config: {
        ...mockAccountConfig("", "Old club address", "ada@club.example", "club.example", []),
      },
      duplicate_of: null,
      issues: [
        issue(
          "secret-not-imported",
          "warning",
          { label: "ada@club.example" },
          "the password stays in Thunderbird",
        ),
        issue(
          "account-not-expressible",
          "warning",
          { label: "ada@club.example" },
          "mail.server.server2.check_all_folders_for_new has no equivalent",
        ),
      ],
    },
  ];
}

function ruleProposals(): RuleProposal[] {
  const base = sampleRule();
  return [
    {
      key: "account1:0",
      account_key: "account1",
      rule: { ...base, id: "", name: "Newsletters", source: null },
      complete: true,
      issues: [],
    },
    {
      key: "account1:1",
      account_key: "account1",
      rule: {
        ...base,
        id: "",
        name: "Junk by size",
        enabled: false,
        actions: [{ type: "tag", tag: "big" }],
        source: null,
      },
      complete: false,
      issues: [
        issue(
          "filter-not-expressible",
          "warning",
          { label: "Junk by size" },
          "condition 'size is greater than' has no rule equivalent",
        ),
      ],
    },
  ];
}

function previewOf(core: MockCore, source: ImportSource): ImportPreview {
  const name = baseName(source.path).replace(/\.[^.]+$/, "") || source.path;
  const counts = emptyCounts();
  const preview: ImportPreview = {
    source: structuredClone(source),
    folders: [],
    collections: [],
    counts,
    accounts: [],
    rules: [],
    issues: [],
    issues_truncated: 0,
  };
  switch (source.format) {
    case "mbox":
      preview.folders = [{ path: name, name, messages: 120, size_bytes: 6_400_000 }];
      preview.issues = [
        issue(
          "truncated",
          "skipped",
          { file: source.path, index: 120, offset: 6_399_000 },
          "the last message ends without a newline",
        ),
      ];
      break;
    case "eml-folder":
      preview.folders = [{ path: name, name, messages: 25, size_bytes: 900_000 }];
      break;
    case "maildir":
      preview.folders = [
        { path: "INBOX", name: "INBOX", messages: 340, size_bytes: 21_000_000 },
        { path: "Archive", name: "Archive", messages: 1200, size_bytes: 88_000_000 },
        { path: "Archive/2024", name: "2024", messages: 410, size_bytes: 30_500_000 },
      ];
      preview.issues = [
        issue(
          "unknown-flag",
          "warning",
          { folder: "INBOX", index: 12, label: "Quarterly numbers" },
          "flag 'a' (keyword) is not defined",
        ),
      ];
      break;
    case "thunderbird-profile":
      preview.folders = [
        { path: "Local Folders/Inbox", name: "Inbox", messages: 87, size_bytes: 5_200_000 },
        {
          path: "Local Folders/Archives/2024",
          name: "2024",
          messages: 512,
          size_bytes: 41_000_000,
        },
      ];
      preview.collections = [
        {
          key: "jsaddrbook://abook.sqlite",
          name: "Personal Address Book",
          kind: "address-book",
          items: 42,
        },
        {
          key: "jsaddrbook://history.sqlite",
          name: "Collected Addresses",
          kind: "address-book",
          items: 130,
        },
      ];
      preview.accounts = accountProposals(core);
      preview.rules = ruleProposals();
      counts.contacts = 172;
      counts.accounts = preview.accounts.length;
      counts.rules = preview.rules.length;
      break;
    case "ics":
      preview.collections = [{ key: source.path, name, kind: "calendar", items: 14 }];
      counts.events = 12;
      counts.tasks = 2;
      break;
    case "vcard":
      preview.collections = [{ key: source.path, name, kind: "address-book", items: 32 }];
      counts.contacts = 30;
      counts.groups = 2;
      break;
  }
  counts.folders = preview.folders.length;
  counts.messages = preview.folders.reduce((n, f) => n + f.messages, 0);
  return preview;
}

function checkSource(state: PortabilityMockState, path: string): Response | null {
  if (!isAbsolutePath(path)) {
    return fail(
      "invalid_request",
      `not an absolute path: ${path}`,
      "portability-path-not-absolute",
    );
  }
  if (state.missing.includes(path)) {
    return fail("not_found", `${path} does not exist`, "portability-path-missing");
  }
  return null;
}

// ---------------------------------------------------------------------------
// Jobs

function now(): string {
  return new Date().toISOString();
}

function newReport(id: string, kind: JobKind, source: string | null, target: string | null) {
  const report: JobReport = {
    job_id: id,
    kind,
    state: "completed",
    started_at: now(),
    finished_at: now(),
    source,
    target,
    counts: emptyCounts(),
    issues: [],
    issues_truncated: 0,
    created_folders: [],
    accounts: [],
    rules: [],
    backup: null,
    restore: null,
    error: null,
    message: null,
  };
  return report;
}

function startJob(
  core: MockCore,
  kind: JobKind,
  path: string,
  total: number,
  phases: JobPhase[],
  report: JobReport,
): Response {
  const state = core.data.features.portability;
  const status: JobStatus = {
    job_id: report.job_id,
    kind,
    state: "running",
    started_at: report.started_at,
    path,
    progress: {
      phase: "queued",
      done: 0,
      total: null,
      bytes_done: 0,
      bytes_total: null,
      current: null,
    },
    report: null,
  };
  state.jobs[report.job_id] = { status, phases, step: 0, total, report };
  if (state.autoProgress) core.later(() => tick(core, report.job_id));
  return { type: "PortabilityJob", job: structuredClone(status) };
}

/** `JobReport::finished_state`: problems other than duplicates. */
function finishedState(report: JobReport): JobReport["state"] {
  return report.issues.some((i) => i.code !== "duplicate") || report.counts.skipped > 0
    ? "completed-with-issues"
    : "completed";
}

function finish(core: MockCore, job: MockJob, cancelled: boolean): void {
  const report = job.report;
  report.finished_at = now();
  report.state = cancelled ? "cancelled" : report.error ? "failed" : finishedState(report);
  job.status.state = report.state;
  job.status.report = structuredClone(report);
  core.emit({ type: "PortabilityJobFinished", report: structuredClone(report) });
}

/** One progress step of a job (the core sends at most a few per second). */
function tick(core: MockCore, jobId: string): void {
  const state = core.data.features.portability;
  const job = state.jobs[jobId];
  if (!job || job.status.state !== "running") return;
  const perPhase = Math.max(1, state.stepsPerPhase);
  const steps = job.phases.length * perPhase;
  job.step++;
  if (job.step > steps) {
    finish(core, job, false);
    return;
  }
  const phase = job.phases[Math.floor((job.step - 1) / perPhase)] ?? "finishing";
  const progress = job.status.progress;
  progress.phase = phase;
  if (phase !== "scanning" && phase !== "queued") {
    progress.total = job.total;
    progress.done = Math.min(job.total, Math.round((job.total * job.step) / steps));
    progress.bytes_total = job.total * 50_000;
    progress.bytes_done = progress.done * 50_000;
  }
  progress.current = job.report.source ? baseName(job.report.source) : baseName(job.status.path);
  core.emit({ type: "PortabilityJobProgress", job_id: jobId, progress: structuredClone(progress) });
  if (state.autoProgress) core.later(() => tick(core, jobId));
}

/** Runs every running job one step (tests with `autoProgress` off). */
export function advancePortabilityJobs(core: MockCore, steps = 1): void {
  for (let i = 0; i < steps; i++) {
    for (const id of Object.keys(core.data.features.portability.jobs)) tick(core, id);
  }
}

/** Runs every running job to its end. */
export function finishPortabilityJobs(core: MockCore): void {
  for (let i = 0; i < 100; i++) {
    const running = Object.values(core.data.features.portability.jobs).some(
      (j) => j.status.state === "running",
    );
    if (!running) return;
    advancePortabilityJobs(core);
  }
}

function startImport(core: MockCore, source: ImportSource, options: ImportOptions): Response {
  const preview = previewOf(core, source);
  const id = core.nextId("job-");
  const report = newReport(id, "import", source.path, null);
  const folders =
    options.folders === null
      ? preview.folders
      : preview.folders.filter((f) => options.folders?.includes(f.path));
  const collections =
    options.collections === null
      ? preview.collections
      : preview.collections.filter((c) => options.collections?.includes(c.key));
  const c = report.counts;
  c.folders = folders.length;
  c.messages = folders.reduce((n, f) => n + f.messages, 0);
  for (const col of collections) {
    if (col.kind === "address-book") c.contacts += col.items;
    else if (source.format === "ics") {
      c.events += preview.counts.events;
      c.tasks += preview.counts.tasks;
    } else c.events += col.items;
  }
  if (source.format === "vcard") c.groups = preview.counts.groups;
  if (c.messages > 0) {
    report.issues.push(
      issue(
        "malformed-message",
        "skipped",
        { file: source.path, folder: folders[0]?.path ?? null, index: 17, offset: 812_331 },
        "no header block before the body",
      ),
    );
    c.skipped++;
    c.messages--;
    if (options.skip_duplicates) {
      report.issues.push(
        issue(
          "duplicate",
          "skipped",
          { folder: folders[0]?.path ?? null, index: 3, label: "Welcome to the list" },
          "same Message-ID, date and size in the target folder",
        ),
      );
      c.duplicates++;
      c.messages--;
    }
  }
  report.issues.push(...preview.issues);
  report.accounts = preview.accounts;
  report.rules = preview.rules;
  c.accounts = preview.accounts.length;
  c.rules = preview.rules.length;
  const total = Math.max(1, c.messages + c.contacts + c.events + c.tasks);
  return startJob(
    core,
    "import",
    source.path,
    total,
    ["scanning", "reading", "writing", "finishing"],
    report,
  );
}

function exportCount(core: MockCore, source: ExportSource): number {
  switch (source.type) {
    case "folders":
      return source.folder_ids.reduce(
        (n, id) => n + core.data.messages.filter((m) => m.folder_id === id).length,
        0,
      );
    case "messages":
      return source.message_ids.length;
    case "calendars":
      return source.calendar_ids.length * 12;
    case "address-books":
      return source.address_book_ids.length * 30;
  }
}

function sourceIsEmpty(source: ExportSource): boolean {
  switch (source.type) {
    case "folders":
      return source.folder_ids.length === 0;
    case "messages":
      return source.message_ids.length === 0;
    case "calendars":
      return source.calendar_ids.length === 0;
    case "address-books":
      return source.address_book_ids.length === 0;
  }
}

function startExport(
  core: MockCore,
  source: ExportSource,
  format: ExportFormat,
  path: string,
  overwrite: boolean,
): Response {
  const state = core.data.features.portability;
  if (!exportAccepts(format, source.type) || sourceIsEmpty(source)) {
    return fail(
      "invalid_request",
      `${format} cannot write ${source.type}`,
      "portability-export-mismatch",
    );
  }
  if (!isAbsolutePath(path)) {
    return fail(
      "invalid_request",
      `not an absolute path: ${path}`,
      "portability-path-not-absolute",
    );
  }
  if (!overwrite && state.existing.includes(path)) {
    return fail("conflict", `${path} exists`, "portability-target-exists");
  }
  const id = core.nextId("job-");
  const report = newReport(id, "export", null, path);
  const count = exportCount(core, source);
  if (source.type === "folders" || source.type === "messages") {
    report.counts.messages = count;
    report.counts.folders = source.type === "folders" ? source.folder_ids.length : 0;
    if (count > 2) {
      report.issues.push(
        issue(
          "malformed-message",
          "warning",
          { folder: source.type === "folders" ? (source.folder_ids[0] ?? null) : null, index: 2 },
          "a header line was folded to fit the format",
        ),
      );
    }
  } else if (source.type === "calendars") {
    report.counts.events = count;
  } else {
    report.counts.contacts = count;
  }
  report.counts.files = Math.max(1, report.counts.folders);
  state.existing.push(path);
  return startJob(
    core,
    "export",
    path,
    Math.max(1, count),
    ["reading", "writing", "finishing"],
    report,
  );
}

// ---------------------------------------------------------------------------
// Backups

const ENTRY_NAMES: Record<string, string[]> = {
  settings: ["settings/app-settings.json"],
  theme: ["config/theme.kdl"],
  layout: ["config/layout.kdl"],
  rules: ["config/rules.kdl", "settings/saved-searches.json"],
  accounts: ["config/accounts.kdl"],
  "local-mail": ["mail/local-folders/Inbox.mbox", "mail/local-folders/Archives.mbox"],
  keys: ["keys/keyring.json"],
  "pim-local": ["pim/calendar.ics", "pim/contacts.vcf", "pim/tasks.ics"],
  plugins: ["plugins/follow-up.nailplugin", "plugins/state.json"],
};

function manifestFor(core: MockCore, contents: BackupContents): BackupManifest {
  const entries = contents.items.flatMap((item) =>
    (ENTRY_NAMES[item] ?? []).map((name, i) => ({
      item,
      name,
      size_bytes: 2_000 + name.length * 997 + i * 31_000,
      sha256: (name.length.toString(16) + "ab12cd34".repeat(8)).slice(0, 64),
    })),
  );
  const counts = emptyCounts();
  counts.files = entries.length;
  if (contents.items.includes("local-mail")) {
    counts.messages = 312;
    counts.folders = 2;
  }
  if (contents.items.includes("pim-local")) {
    counts.events = 48;
    counts.contacts = 61;
    counts.tasks = 9;
  }
  const accounts = contents.items.includes("accounts")
    ? core.data.accountsConfig.accounts.map((a) => a.id)
    : [];
  if (contents.items.includes("accounts")) counts.accounts = accounts.length;
  if (contents.items.includes("rules")) counts.rules = 3;
  return {
    format_version: 1,
    app_version: "1.0.0-rc.1",
    schema_version: 15,
    created_at: now(),
    contents: { items: [...contents.items] },
    entries,
    counts,
    accounts,
    plugins: contents.items.includes("plugins") ? ["follow-up"] : [],
  };
}

function createBackup(
  core: MockCore,
  path: string,
  contents: BackupContents,
  passphrase: string,
): Response {
  const state = core.data.features.portability;
  const problem = [...backupContentsIssues(contents), ...passphraseIssues(passphrase)][0];
  if (problem) return fail("invalid_request", problem, `backup-issue-${problem}`);
  if (!isAbsolutePath(path)) {
    return fail(
      "invalid_request",
      `not an absolute path: ${path}`,
      "portability-path-not-absolute",
    );
  }
  const manifest = manifestFor(core, contents);
  state.backups[path] = { passphrase, manifest, damaged: [] };
  const id = core.nextId("job-");
  const report = newReport(id, "backup", null, path);
  report.counts = structuredClone(manifest.counts);
  report.backup = structuredClone(manifest);
  return startJob(
    core,
    "backup",
    path,
    manifest.entries.length,
    ["reading", "writing", "verifying", "finishing"],
    report,
  );
}

function openBackup(state: PortabilityMockState, path: string, passphrase: string) {
  const backup = state.backups[path];
  if (!backup) {
    return {
      error: state.missing.includes(path)
        ? fail("not_found", `${path} does not exist`, "portability-path-missing")
        : fail("auth_failed", "not a Nail backup", "backup-wrong-passphrase"),
    };
  }
  if (backup.passphrase !== passphrase) {
    return { error: fail("auth_failed", "wrong passphrase", "backup-wrong-passphrase") };
  }
  return { backup };
}

function damageIssues(backup: MockBackup): ItemIssue[] {
  return backup.damaged.map((name) =>
    issue(
      "checksum-mismatch",
      "skipped",
      { file: name, label: name },
      "SHA-256 differs from the manifest",
    ),
  );
}

function verifyBackup(core: MockCore, path: string, passphrase: string): Response {
  const state = core.data.features.portability;
  const opened = openBackup(state, path, passphrase);
  if (opened.error) return opened.error;
  const id = core.nextId("job-");
  const report = newReport(id, "verify-backup", path, null);
  report.backup = structuredClone(opened.backup.manifest);
  report.counts.files = opened.backup.manifest.entries.length - opened.backup.damaged.length;
  report.issues = damageIssues(opened.backup);
  report.counts.skipped = report.issues.length;
  return startJob(
    core,
    "verify-backup",
    path,
    opened.backup.manifest.entries.length,
    ["verifying", "finishing"],
    report,
  );
}

function restoreActions(backup: MockBackup, contents: BackupContents | null) {
  const chosen = contents?.items ?? backup.manifest.contents.items;
  const accountsNeedingSecrets = chosen.includes("accounts") ? [...backup.manifest.accounts] : [];
  const actions: RestoreAction[] = BACKUP_ITEMS.filter((i) =>
    backup.manifest.contents.items.includes(i),
  ).map((item) => {
    const entries = backup.manifest.entries.filter((e) => e.item === item).length;
    if (!chosen.includes(item)) {
      return { item, effect: "skip", added: 0, replaced: 0, unchanged: entries };
    }
    const replaces = ["settings", "theme", "layout", "rules", "accounts"].includes(item);
    const action: RestoreAction = replaces
      ? { item, effect: "replace", added: 0, replaced: entries, unchanged: 0 }
      : item === "local-mail"
        ? { item, effect: "merge", added: 280, replaced: 0, unchanged: 32 }
        : item === "pim-local"
          ? { item, effect: "merge", added: 20, replaced: 0, unchanged: 98 }
          : { item, effect: "add", added: entries, replaced: 0, unchanged: 0 };
    if (item === "accounts" && accountsNeedingSecrets.length > 0) {
      action.note = {
        id: "portability-restore-note-passwords",
        args: { count: accountsNeedingSecrets.length },
      };
    }
    return action;
  });
  return {
    actions,
    accountsNeedingSecrets,
    restartRequired: chosen.includes("settings") || chosen.includes("plugins"),
  };
}

function previewRestore(
  core: MockCore,
  path: string,
  passphrase: string,
  contents: BackupContents | null,
): Response {
  const state = core.data.features.portability;
  const opened = openBackup(state, path, passphrase);
  if (opened.error) return opened.error;
  if (contents && backupContentsIssues(contents).length > 0) {
    const problem = backupContentsIssues(contents)[0];
    return fail("invalid_request", String(problem), `backup-issue-${problem}`);
  }
  const planned = restoreActions(opened.backup, contents);
  const plan: RestorePlan = {
    manifest: structuredClone(opened.backup.manifest),
    actions: planned.actions,
    issues: damageIssues(opened.backup),
    accounts_needing_secrets: planned.accountsNeedingSecrets,
    restart_required: planned.restartRequired,
  };
  return { type: "RestorePlan", plan };
}

function startRestore(
  core: MockCore,
  path: string,
  passphrase: string,
  contents: BackupContents | null,
): Response {
  const state = core.data.features.portability;
  const opened = openBackup(state, path, passphrase);
  if (opened.error) return opened.error;
  const planned = restoreActions(opened.backup, contents);
  const id = core.nextId("job-");
  const report = newReport(id, "restore", path, null);
  report.backup = structuredClone(opened.backup.manifest);
  report.issues = damageIssues(opened.backup);
  report.counts.skipped = report.issues.length;
  report.counts.files = planned.actions.reduce((n, a) => n + a.added + a.replaced, 0);
  report.restore = {
    actions: planned.actions,
    config_backups: planned.actions
      .filter((a) => a.effect === "replace")
      .map((a) => `${a.item}-${Date.now()}`),
    accounts_needing_secrets: planned.accountsNeedingSecrets,
    restart_required: planned.restartRequired,
  };
  return startJob(
    core,
    "restore",
    path,
    Math.max(1, report.counts.files),
    ["reading", "writing", "finishing"],
    report,
  );
}

// ---------------------------------------------------------------------------
// Diagnostics

function diagnosticsEntries(core: MockCore, options: DiagnosticsOptions): DiagnosticsEntry[] {
  const redacted = options.redact_addresses ? (["addresses", "subjects"] as const) : [];
  const out: DiagnosticsEntry[] = [];
  const text = (
    name: string,
    item: DiagnosticsEntry["item"],
    preview: string,
    extra: DiagnosticsEntry["redactions"] = [],
  ) =>
    out.push({
      name,
      item,
      size_bytes: preview.length * 7,
      redactions: ["secrets", ...extra],
      preview,
    });
  for (const item of options.items) {
    switch (item) {
      case "system-info":
        text(
          "system.json",
          item,
          '{\n  "app": "1.0.0-rc.1",\n  "os": "Windows 11 (26100)",\n  "webview": "WebView2 141"\n}',
          ["paths"],
        );
        break;
      case "logs":
        text(
          "logs/nail.log",
          item,
          `2026-10-02T09:00:01Z INFO sync: account ${options.redact_addresses ? "<account-1>" : "acc-personal"} online\n2026-10-02T09:00:05Z WARN imap: idle restarted after timeout`,
          [...redacted, "paths"],
        );
        break;
      case "config":
        text(
          "config/accounts.kdl",
          item,
          `account "acc-personal" {\n    email "${options.redact_addresses ? "<address-1>" : "ada@example.org"}"\n    password "<removed>"\n}`,
          [...redacted],
        );
        text("config/layout.kdl", item, 'layout "default" {\n    // …\n}');
        break;
      case "account-status":
        text(
          "accounts/status.json",
          item,
          JSON.stringify(
            core.data.statuses.map((s) => ({ account: s.account_id, state: s.state })),
            null,
            2,
          ),
          [...redacted],
        );
        break;
      case "store-stats":
        text(
          "store/stats.json",
          item,
          `{\n  "schema": 15,\n  "messages": ${core.data.messages.length}\n}`,
        );
        break;
      case "operation-queue":
        text("store/operation-queue.json", item, '{\n  "pending": 0,\n  "failed": 0\n}');
        break;
      case "plugins":
        text("plugins.json", item, "[]");
        break;
      case "mail-headers":
        for (const id of options.message_ids) {
          const m = core.data.messages.find((x) => x.id === id);
          text(
            `mail/${id.replace(/[^a-zA-Z0-9.-]+/g, "_")}.headers.txt`,
            item,
            `Subject: ${m?.subject ?? ""}\nFrom: ${m?.from?.email ?? ""}`,
          );
        }
        break;
      case "mail-content":
        for (const id of options.message_ids) {
          const m = core.data.messages.find((x) => x.id === id);
          out.push({
            name: `mail/${id.replace(/[^a-zA-Z0-9.-]+/g, "_")}.eml`,
            item,
            size_bytes: 12_000,
            redactions: ["secrets"],
            preview: `Subject: ${m?.subject ?? ""}\n\n${m?.preview ?? ""}`,
          });
        }
        break;
    }
  }
  return out;
}

function diagnosticsManifest(core: MockCore, options: DiagnosticsOptions): DiagnosticsManifest {
  return {
    created_at: now(),
    app_version: "1.0.0-rc.1",
    options: structuredClone(options),
    entries: diagnosticsEntries(core, options),
  };
}

function checkDiagnostics(options: DiagnosticsOptions): Response | null {
  const problem = diagnosticsIssues(options)[0];
  return problem ? fail("invalid_request", problem, `diagnostics-issue-${problem}`) : null;
}

// ---------------------------------------------------------------------------
// File dialog

function defaultPick(pick: PathPick): string {
  const ext = pick.filters[0]?.extensions[0];
  switch (pick.kind) {
    case "open-folder":
      return "C:\\Users\\Ada\\Mail\\Maildir";
    case "open-file":
      return `C:\\Users\\Ada\\Documents\\${ext === "nailbackup" ? "Nail backup.nailbackup" : `mail.${ext ?? "mbox"}`}`;
    case "save-file":
      return `C:\\Users\\Ada\\Documents\\${pick.default_path ?? `export.${ext ?? "mbox"}`}`;
  }
}

export function answerPortability(core: MockCore, request: PortabilityRequest): Response {
  const state = core.data.features.portability;
  switch (request.type) {
    case "PickPath": {
      if (!state.desktop) return notReady(request, "desktop-unavailable");
      const queued = state.pickAnswers.length > 0 ? state.pickAnswers.shift() : undefined;
      return {
        type: "PathPicked",
        path: queued === undefined ? defaultPick(request.pick) : queued,
      };
    }
    case "FindImportSources":
      return {
        type: "ImportSources",
        sources: state.desktop
          ? [
              {
                source: {
                  format: "thunderbird-profile",
                  path: "C:\\Users\\Ada\\AppData\\Roaming\\Thunderbird\\Profiles\\x1y2z3.default-release",
                },
                label: "default-release",
                is_default: true,
                last_used_at: "2026-09-30T18:12:00Z",
              },
            ]
          : [],
      };
    case "PreviewImport":
      return (
        checkSource(state, request.source.path) ?? {
          type: "ImportPreview",
          preview: previewOf(core, request.source),
        }
      );
    case "StartImport":
      return (
        checkSource(state, request.source.path) ??
        startImport(core, request.source, request.options)
      );
    case "StartExport":
      return startExport(
        core,
        request.source,
        request.format,
        request.path,
        request.options.overwrite,
      );
    case "CreateBackup":
      return createBackup(core, request.path, request.contents, request.passphrase);
    case "InspectBackup": {
      const opened = openBackup(state, request.path, request.passphrase);
      return (
        opened.error ?? {
          type: "BackupManifest",
          manifest: structuredClone(opened.backup.manifest),
        }
      );
    }
    case "VerifyBackup":
      return verifyBackup(core, request.path, request.passphrase);
    case "PreviewRestore":
      return previewRestore(core, request.path, request.passphrase, request.contents);
    case "StartRestore":
      return startRestore(core, request.path, request.passphrase, request.contents);
    case "ListPortabilityJobs":
      return {
        type: "PortabilityJobs",
        jobs: Object.values(state.jobs).map((j) => structuredClone(j.status)),
      };
    case "GetPortabilityJob": {
      const job = state.jobs[request.job_id];
      return job
        ? { type: "PortabilityJob", job: structuredClone(job.status) }
        : fail("not_found", `job ${request.job_id} is unknown`, "portability-job-gone");
    }
    case "CancelPortabilityJob": {
      const job = state.jobs[request.job_id];
      if (!job)
        return fail("not_found", `job ${request.job_id} is unknown`, "portability-job-gone");
      if (job.status.state === "running") finish(core, job, true);
      return { type: "PortabilityJob", job: structuredClone(job.status) };
    }
    case "PreviewDiagnostics":
      return (
        checkDiagnostics(request.options) ?? {
          type: "DiagnosticsPreview",
          manifest: diagnosticsManifest(core, request.options),
        }
      );
    case "CreateDiagnosticsBundle": {
      const problem = checkDiagnostics(request.options);
      if (problem) return problem;
      if (!isAbsolutePath(request.path)) {
        return fail("invalid_request", "not an absolute path", "portability-path-not-absolute");
      }
      const manifest = diagnosticsManifest(core, request.options);
      state.bundles.push(request.path);
      return {
        type: "DiagnosticsBundle",
        bundle: {
          path: request.path,
          manifest,
          size_bytes: manifest.entries.reduce((n, e) => n + e.size_bytes, 0) + 512,
          sha256: "9f2c4e1a7b3d5f6e8a0c2e4b6d8f0a1c3e5b7d9f1a3c5e7b9d1f3a5c7e9b1d3f",
        },
      };
    }
  }
}
