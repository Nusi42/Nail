/**
 * The mock core's connection log (ADR 0190): one ring per scope of
 * already-redacted demo lines (a sign-in, capabilities, the inbox opened,
 * messages fetched as `{n bytes}`), more on every `SyncNow`
 * (`recordMockSync`), the disk setting, stored sync errors and skipped
 * messages, and the application log. `failSyncWith` makes the next sync of
 * an account fail to open a folder with details, like the core does.
 * `../../api/mock.ts` routes exactly these requests here.
 */
import type {
  AccountId,
  AppLog,
  ConnectionLogEntry,
  ConnectionLogScope,
  ConnectionLogSettings,
  ErrorDetails,
  Request,
  Response,
  SkippedMessage,
  SyncErrorRecord,
  WireDirection,
  WireLevel,
  WireProtocol,
} from "../../api/generated";
import type { MockCore } from "../../api/mockMail";
import { scopeKey } from "./model";

/** Entries the mock keeps per scope (the core answers its own `capacity`). */
const MOCK_CAPACITY = 2000;

/** The requests this feature answers. */
export const CONNLOG_REQUESTS = [
  "GetConnectionLog",
  "ClearConnectionLog",
  "ExportConnectionLog",
  "GetConnectionLogSettings",
  "SetConnectionLogSettings",
  "ListSyncErrors",
  "GetAppLog",
] as const satisfies readonly Request["type"][];

export type ConnlogRequest = Extract<Request, { type: (typeof CONNLOG_REQUESTS)[number] }>;

export function isConnlogRequest(request: Request): request is ConnlogRequest {
  return (CONNLOG_REQUESTS as readonly string[]).includes(request.type);
}

interface MockLog {
  entries: ConnectionLogEntry[];
  lastSeq: number;
  dropped: number;
}

/** Mock-core state of this feature. */
export interface ConnlogMockState {
  logs: Record<string, MockLog>;
  settings: ConnectionLogSettings;
  errors: SyncErrorRecord[];
  skipped: SkippedMessage[];
  appLog: AppLog;
  /** Files `ExportConnectionLog` "wrote", by path. */
  exports: Record<string, string>;
  /** The folder the next sync of an account fails to open. */
  failures: Map<AccountId, string>;
  connections: number;
}

export function createConnlogMockState(): ConnlogMockState {
  return {
    logs: {},
    settings: { keep_on_disk: false },
    errors: [],
    skipped: [],
    appLog: {
      path: null,
      lines: [
        "2026-10-08T09:12:00.120Z  INFO nail_core::engine: engine started account=acc-personal",
        "2026-10-08T09:12:01.480Z  INFO nail_core::sync: sync finished account=acc-personal",
      ],
    },
    exports: {},
    failures: new Map(),
    connections: 0,
  };
}

/** One line as `nail_api::connlog::ConnectionLogEntry::line` writes it. */
function line(entry: ConnectionLogEntry): string {
  const marker =
    entry.direction === "sent"
      ? "C:"
      : entry.direction === "received"
        ? "S:"
        : entry.level === "info"
          ? "--"
          : entry.level === "warning"
            ? "!!"
            : "XX";
  return `${entry.at} ${entry.protocol} #${String(entry.connection)} ${marker} ${entry.text}`;
}

/** The core's suggested file name (`nail_connlog::export_name`). */
function exportName(scope: ConnectionLogScope): string {
  const clean = (id: string) => id.slice(0, 64).replace(/[^A-Za-z0-9_-]/g, "_");
  const stem =
    scope.kind === "account"
      ? `account-${clean(scope.account_id)}`
      : scope.kind === "pim-source"
        ? `source-${clean(scope.source_id)}`
        : "setup";
  return `nail-connection-log-${stem}.txt`;
}

function logOf(state: ConnlogMockState, scope: ConnectionLogScope): MockLog {
  const key = scopeKey(scope);
  let log = state.logs[key];
  if (!log) {
    log = { entries: [], lastSeq: 0, dropped: 0 };
    state.logs[key] = log;
  }
  return log;
}

type Line = [WireDirection, string, WireLevel?];

/** Appends lines of one connection to a scope's log and announces them. */
export function appendMockLines(
  core: MockCore,
  scope: ConnectionLogScope,
  protocol: WireProtocol,
  lines: readonly Line[],
): void {
  const state = core.data.features.connlog;
  const log = logOf(state, scope);
  state.connections += 1;
  const connection = state.connections;
  for (const [direction, text, level = "info"] of lines) {
    log.lastSeq += 1;
    log.entries.push({
      seq: log.lastSeq,
      at: new Date().toISOString(),
      protocol,
      connection,
      direction,
      level,
      text,
    });
  }
  while (log.entries.length > MOCK_CAPACITY) {
    log.entries.shift();
    log.dropped += 1;
  }
  core.emit({ type: "ConnectionLogChanged", scope, last_seq: log.lastSeq });
}

/** The masked address of an account (`a***@example.org`). */
function masked(core: MockCore, accountId: AccountId): string {
  const email = core.data.accounts.find((a) => a.id === accountId)?.email ?? "user@example.org";
  const [local = "", domain = ""] = email.split("@");
  return `${local.slice(0, 1)}***@${domain}`;
}

/** The lines of a sign-in and an inbox sync. */
function syncLines(core: MockCore, accountId: AccountId): Line[] {
  return [
    ["info", "connecting to imap.example.org:993"],
    [
      "info",
      "TLS TLSv1_3 TLS13_AES_256_GCM_SHA384; certificate CN=imap.example.org, issued by CN=Example CA, valid until 2027-03-01, SHA-256 1a2b3c4d5e6f7081",
    ],
    ["received", "* OK [CAPABILITY IMAP4rev1 SASL-IR AUTH=PLAIN IDLE] Example IMAP ready."],
    ["sent", `A0001 LOGIN "${masked(core, accountId)}" <redacted>`],
    ["received", "A0001 OK [CAPABILITY IMAP4rev1 IDLE CONDSTORE UIDPLUS MOVE] Logged in"],
    ["sent", 'A0002 LIST "" "*"'],
    ["received", '* LIST (\\HasNoChildren) "/" "INBOX"'],
    ["received", "A0002 OK List completed"],
    ["sent", 'A0003 EXAMINE "INBOX" (CONDSTORE)'],
    ["received", "* 12 EXISTS"],
    ["received", "* OK [UIDVALIDITY 1700000000] UIDs valid"],
    ["received", "A0003 OK [READ-ONLY] Examine completed"],
    ["sent", "A0004 UID FETCH 12 (UID FLAGS INTERNALDATE RFC822.SIZE MODSEQ BODY.PEEK[])"],
    [
      "received",
      '* 12 FETCH (UID 12 FLAGS (\\Seen) INTERNALDATE "08-Oct-2026 09:10:00 +0200" RFC822.SIZE 4211 MODSEQ (77) BODY[] {4211 bytes})',
    ],
    ["received", "A0004 OK Fetch completed"],
  ];
}

/** The details of a folder that could not be opened. */
export function selectFailure(folder: string, logSeq: number | null): ErrorDetails {
  return {
    code: "not_found",
    protocol: "imap",
    step: "select",
    folder,
    command: `A0005 EXAMINE "${folder}" (CONDSTORE)`,
    server_text: `NO [NONEXISTENT] Mailbox doesn't exist: ${folder}`,
    detail: `server refused: [NONEXISTENT] Mailbox doesn't exist: ${folder}`,
    at: new Date().toISOString(),
    ...(logSeq === null ? {} : { log_seq: logSeq }),
  };
}

/** Makes the next sync of `accountId` fail to open `folder`. */
export function failSyncWith(core: MockCore, accountId: AccountId, folder: string): void {
  core.data.features.connlog.failures.set(accountId, folder);
}

/**
 * What `SyncNow` adds: the sync's lines and, when a failure is set, the
 * folder's refusal, its stored error and `SyncFailed` with details. Returns
 * whether the sync failed.
 */
export function recordMockSync(core: MockCore, accountId: AccountId): boolean {
  const state = core.data.features.connlog;
  const scope: ConnectionLogScope = { kind: "account", account_id: accountId };
  appendMockLines(core, scope, "imap", syncLines(core, accountId));
  const folder = state.failures.get(accountId);
  if (folder === undefined) return false;
  state.failures.delete(accountId);
  const details = selectFailure(folder, null);
  appendMockLines(core, scope, "imap", [
    ["sent", details.command ?? ""],
    ["received", details.server_text ?? "", "warning"],
  ]);
  const withSeq: ErrorDetails = { ...details, log_seq: logOf(state, scope).lastSeq };
  state.errors = [
    ...state.errors.filter((e) => !(e.account_id === accountId && e.details.folder === folder)),
    { account_id: accountId, folder_id: `${accountId}/${folder}`, details: withSeq },
  ];
  core.emit({ type: "SyncErrorsChanged", account_id: accountId });
  core.emit({
    type: "SyncFailed",
    account_id: accountId,
    code: withSeq.code,
    message: `1 folder(s) could not be synchronized (${folder}); first error: ${withSeq.detail}`,
    localized: { id: "sync-error-folders", args: { count: 1, folders: folder } },
    details: withSeq,
  });
  return true;
}

function seeded(core: MockCore, scope: ConnectionLogScope): MockLog {
  const state = core.data.features.connlog;
  const log = logOf(state, scope);
  if (log.lastSeq === 0 && scope.kind === "account") {
    appendMockLines(core, scope, "imap", syncLines(core, scope.account_id));
  }
  return log;
}

export function answerConnlog(core: MockCore, request: ConnlogRequest): Response {
  const state = core.data.features.connlog;
  switch (request.type) {
    case "GetConnectionLog": {
      const log = seeded(core, request.scope);
      let entries = log.entries.filter((e) => request.after === undefined || e.seq > request.after);
      if (request.limit !== undefined && entries.length > request.limit) {
        entries = entries.slice(entries.length - request.limit);
      }
      return {
        type: "ConnectionLog",
        log: {
          scope: request.scope,
          entries: structuredClone(entries),
          last_seq: log.lastSeq,
          dropped: log.dropped,
          capacity: MOCK_CAPACITY,
          keep_on_disk: state.settings.keep_on_disk,
        },
      };
    }
    case "ClearConnectionLog": {
      const log = logOf(state, request.scope);
      log.dropped += log.entries.length;
      log.entries = [];
      core.emit({ type: "ConnectionLogChanged", scope: request.scope, last_seq: log.lastSeq });
      return { type: "Done" };
    }
    case "ExportConnectionLog": {
      const text = logOf(state, request.scope)
        .entries.map((e) => `${line(e)}\n`)
        .join("");
      const file_name = exportName(request.scope);
      if (request.path === undefined) return { type: "ConnectionLogExport", text, file_name };
      if (!core.data.features.portability.desktop) {
        return {
          type: "Error",
          code: "unsupported",
          message: "saving files needs the desktop app",
          localized: { id: "desktop-unavailable" },
        };
      }
      state.exports[request.path] = text;
      return { type: "ConnectionLogExport", path: request.path, file_name };
    }
    case "GetConnectionLogSettings":
      return { type: "ConnectionLogSettings", settings: { ...state.settings } };
    case "SetConnectionLogSettings":
      state.settings = { ...request.settings };
      return { type: "ConnectionLogSettings", settings: { ...state.settings } };
    case "ListSyncErrors": {
      const of = <T extends { account_id: AccountId }>(items: T[]) =>
        items.filter(
          (i) => request.account_id === undefined || i.account_id === request.account_id,
        );
      return {
        type: "SyncErrors",
        errors: structuredClone(of(state.errors)),
        skipped: structuredClone(of(state.skipped)),
      };
    }
    case "GetAppLog":
      return { type: "AppLog", log: structuredClone(state.appLog) };
  }
}
