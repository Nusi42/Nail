/**
 * `@nail/plugin-api/testing` — an in-memory stand-in for Nail's plug-in
 * host, for unit tests of plug-ins (Vitest, `node:test`, …). It implements
 * the global `nail` object with the host's rules: every call needs its
 * permission (and, for mail, a granted account) and rejects with
 * `forbidden` otherwise; writes follow the action policies and ask a
 * scripted user; hooks run with their budgets and their answers are
 * checked like Nail checks them.
 *
 * ```ts
 * const host = createTestHost({ manifest, dialog: () => ({ button: "ok", values: {} }) });
 * const restore = host.install();
 * await plugin.activate(host.activationContext());
 * await host.runCommand("hello");
 * const run = await host.runBeforeSend(outgoing);
 * expect(run.outcome).toBe("vetoed");
 * restore();
 * ```
 */
import type {
  NailApi,
  PluginActionHandler,
  PluginActionKind,
  PluginActivationContext,
  PluginArrivalHook,
  PluginBannerAction,
  PluginBannerActionHandler,
  PluginBeforeSendHook,
  PluginCollection,
  PluginCommandHandler,
  PluginContact,
  PluginContactInput,
  PluginDraftChanges,
  PluginHooksRegistry,
  PluginComposerRegistry,
  PluginAttachmentsRegistry,
  PluginLinksRegistry,
  PluginJobsRegistry,
  PluginMcpRegistry,
  PluginDraftInput,
  PluginEventInput,
  PluginHookMessage,
  PluginHttpRequest,
  PluginJobHandler,
  PluginMarkdownBlockRenderer,
  PluginMcpToolHandler,
  PluginMessageChanges,
  PluginOpenHook,
  PluginPanelProvider,
  PluginPanelRender,
  PluginPreviewRenderer,
  PluginStorageUsage,
  PluginTaskInput,
  PluginWriteResult,
} from "./api";
import type {
  ComposerEdit,
  DraftId,
  JsonValue,
  PluginArrivalResult,
  PluginAttachmentRef,
  PluginBeforeSendResult,
  PluginCommandContext,
  PluginComposerContext,
  PluginDialogResult,
  PluginDialogSpec,
  PluginHookKind,
  PluginJobTrigger,
  PluginLink,
  PluginLogLevel,
  PluginManifest,
  PluginMarkdownBlockResult,
  PluginCalendarEvent,
  PluginMessage,
  PluginMessageBody,
  PluginNotificationSpec,
  PluginOpenResult,
  PluginOutgoingMessage,
  PluginPermission,
  PluginPreview,
  PluginRenderPurpose,
  PluginUiNode,
} from "./generated";
import { toBytes, decodeUtf8, encodeUtf8 } from "./bytes";
import {
  arrivalProblems,
  beforeSendProblems,
  HOOK_TIMEOUT_MS,
  JOB_WALL_CLOCK_MS,
  markdownBlockProblems,
  MAX_PREVIEW_INPUT,
  openProblems,
  previewProblems,
} from "./extensions";
import { createLocalizer } from "./l10n";
import { createPluginApiError } from "./nail";
import { checkTree } from "./tree";

/** Every permission except `network:*` (the default grant of a test host without a manifest). */
export const ALL_FIXED_PERMISSIONS: PluginPermission[] = [
  "mail.read",
  "mail.read.body",
  "calendar.read",
  "ui.dialogs",
  "notifications",
  "storage",
  "mail.organize",
  "mail.send",
  "calendar.write",
  "contacts.write",
  "tasks.write",
  "hooks.arrival",
  "hooks.beforeSend",
  "hooks.open",
  "composer",
  "attachments",
  "background",
  "mcp.expose",
];

/** Permissions a grant can limit to accounts (as in Nail). */
const ACCOUNT_PERMISSIONS = new Set([
  "mail.read",
  "mail.read.body",
  "mail.organize",
  "mail.send",
  "hooks.arrival",
  "hooks.beforeSend",
  "hooks.open",
  "attachments",
]);

/** Kinds that always ask (no policy relaxes them). */
const DESTRUCTIVE = new Set<PluginActionKind>(["trash", "permanent-delete", "pim-delete"]);
/** Kinds that ask unless the user allows them. */
const ASK_BY_DEFAULT = new Set<PluginActionKind>(["send", "invite", "rule"]);

/** Answers a dialog in a test. */
export type DialogResponder = (
  spec: PluginDialogSpec,
) => PluginDialogResult | Promise<PluginDialogResult>;

/** A confirmation the test host's user was asked. */
export interface TestConfirmation {
  /** Its id (`confirmation-1`, …). */
  id: string;
  /** The kind of write. */
  kind: PluginActionKind;
  /** The API call (`nail.mail.send`). */
  operation: string;
  /** What it would do (English, for tests). */
  summary: string;
  /** The items concerned (message ids, draft id, …). */
  targets: string[];
  /** Whether "always allow" could be offered. */
  canAlwaysAllow: boolean;
  /** How the test's user decided. */
  decision: TestDecision;
}

/** How the scripted user answers a confirmation. */
export type TestDecision = "approve" | "deny" | "expire";

/** A write the plug-in made (done or not). */
export interface TestWrite {
  /** The API call (`nail.tasks.create`, `hook.arrival`). */
  operation: string;
  /** The kind of write. */
  kind: PluginActionKind;
  /** The arguments as passed. */
  input: JsonValue;
  /** How it ended. */
  result: PluginWriteResult;
}

/** A canned HTTP response. */
export interface TestHttpResponse {
  /** Status (default 200). */
  status?: number;
  /** Headers. */
  headers?: Record<string, string>;
  /** Body text or bytes. */
  body?: string | Uint8Array;
}

/** A draft in the test host. */
export interface TestDraft extends PluginDraftInput {
  /** Its id. */
  id: DraftId;
  /** Whether it was sent. */
  sent: boolean;
}

/** A task in the test host. */
export interface TestTask extends PluginTaskInput {
  /** Its id. */
  id: string;
  /** Its list. */
  list: string;
  /** Done. */
  completed: boolean;
}

/** An event in the test host. */
export interface TestEvent extends PluginEventInput {
  /** Its id. */
  id: string;
  /** Its calendar. */
  calendarId: string;
}

/** Options of {@link createTestHost}. */
export interface TestHostOptions {
  /**
   * The plug-in's manifest: gives the id, version and (unless
   * `permissions` is set) the granted permissions; commands, panels,
   * hooks and extensions must then be declared in it to be registered,
   * and hook budgets come from it.
   */
  manifest?: PluginManifest;
  /** The granted permissions (default: the manifest's, else {@link ALL_FIXED_PERMISSIONS}). */
  permissions?: PluginPermission[];
  /**
   * The accounts the account-limited permissions are granted for
   * (default: every account).
   */
  accounts?: string[];
  /** Messages `nail.mail` can see. */
  messages?: PluginMessage[];
  /** Bodies by message id. */
  bodies?: Record<string, PluginMessageBody>;
  /** Event occurrences `nail.calendar.events` can see. */
  events?: PluginCalendarEvent[];
  /** Calendars the plug-in may write to (default: one, `calendar`). */
  calendars?: PluginCollection[];
  /** Task lists (default: one, `tasks`). */
  taskLists?: PluginCollection[];
  /** Address books (default: one, `contacts`). */
  addressBooks?: PluginCollection[];
  /** Contacts `nail.contacts.find` can see. */
  contacts?: PluginContact[];
  /** Setting values. */
  settings?: Record<string, JsonValue>;
  /** Nail's language for `nail.l10n` (default `en`). */
  locale?: string;
  /** The plug-in's catalogs: language tag → `.ftl` text (`loadCatalogs` in `@nail/plugin-api/tools` reads them). */
  catalogs?: Record<string, string>;
  /** Answers dialogs (default: dismissed, `{button: null, values: {}}`). */
  dialog?: DialogResponder;
  /** Answers confirmations (default: approve). */
  confirm?: (
    confirmation: Omit<TestConfirmation, "decision">,
  ) => TestDecision | Promise<TestDecision>;
  /** Changed policies: `allow` for a kind that asks by default, `ask` for one that does not (destructive kinds always ask). */
  policies?: Partial<Record<PluginActionKind, "ask" | "allow">>;
  /** Other plug-ins' before-send vetoes for `nail.mail.send` (reasons; empty = none). */
  sendVetoes?: (draft: TestDraft) => string[];
  /** Answers `nail.http.fetch` for allowed hosts (default 404). */
  fetch?: (url: string, request: PluginHttpRequest) => TestHttpResponse | Promise<TestHttpResponse>;
  /** Storage quota in bytes (default 5 MiB). */
  storageQuota?: number;
  /** Hook budgets in ms (default: the manifest's `timeoutMs`, else Nail's defaults). */
  hookTimeoutMs?: Partial<Record<PluginHookKind, number>>;
  /** Wall-clock limit of a job run in ms (default 60 s). */
  jobTimeoutMs?: number;
}

/** A log line recorded by the test host. */
export interface TestLogLine {
  /** Its level. */
  level: PluginLogLevel;
  /** The arguments joined as text. */
  message: string;
}

/** How a hook call ended, as Nail counts it. */
export interface TestHookRun<T> {
  /**
   * `ok` (answered, maybe nothing), `vetoed` (before-send veto), `failed`
   * (threw or answered something invalid), `timed-out` (over its budget)
   * or `skipped` (not registered, not permitted, account not granted).
   */
  outcome: "ok" | "vetoed" | "failed" | "timed-out" | "skipped";
  /** The answer (valid answers only). */
  result?: T;
  /** Why the answer was refused. */
  problems: string[];
  /** The error it threw, or why it was skipped. */
  error?: string;
}

/** How a job run ended. */
export interface TestJobRun {
  /** `ok`, `error` (threw) or `timeout` (over the wall-clock limit). */
  outcome: "ok" | "error" | "timeout";
  /** The error. */
  error?: string;
}

/** The test host (see {@link createTestHost}). */
export interface TestHost {
  /** The `nail` object. */
  readonly api: NailApi;
  /** Dialogs shown, in order. */
  readonly dialogs: PluginDialogSpec[];
  /** Notifications shown, in order. */
  readonly notifications: PluginNotificationSpec[];
  /** Panel updates (`nail.ui.updatePanel`), in order. */
  readonly panelUpdates: { name: string; tree: PluginUiNode[] }[];
  /** Log lines. */
  readonly logs: TestLogLine[];
  /** The storage contents. */
  readonly storage: Map<string, JsonValue>;
  /** Every write, in order (done, denied, expired or vetoed). */
  readonly writes: TestWrite[];
  /** Every confirmation the user was asked, in order. */
  readonly confirmations: TestConfirmation[];
  /** Composer edits, in order. */
  readonly composerEdits: { draftId: DraftId; edit: ComposerEdit }[];
  /** The messages (updated by `nail.mail.update` and arrival actions). */
  readonly messages: Map<string, PluginMessage>;
  /** Drafts created. */
  readonly drafts: Map<string, TestDraft>;
  /** Tasks created. */
  readonly tasks: Map<string, TestTask>;
  /** Events created. */
  readonly calendarEvents: Map<string, TestEvent>;
  /** Contacts created or changed. */
  readonly contactsById: Map<string, PluginContact>;
  /** Registered command names. */
  commandNames(): string[];
  /** Registered panel names. */
  panelNames(): string[];
  /** Registered hooks. */
  hookNames(): PluginHookKind[];
  /** Registered extensions by kind. */
  extensionNames(): {
    composerActions: string[];
    markdownBlocks: string[];
    attachmentPreviews: string[];
    attachmentActions: string[];
    linkActions: string[];
    jobs: string[];
    mcpTools: string[];
  };
  /** Installs `api` as `globalThis.nail`; returns a function restoring the previous value. */
  install(): () => void;
  /** The context the host passes to `activate`. */
  activationContext(): PluginActivationContext;
  /** Runs a registered command like a click in the UI would. */
  runCommand(name: string, ctx?: Partial<PluginCommandContext>): Promise<void>;
  /** Renders a registered panel (checking the tree). */
  renderPanel(name: string, ctx?: Partial<PluginCommandContext>): Promise<PluginUiNode[]>;
  /** Clicks a `button` node in a panel. */
  clickPanelButton(
    name: string,
    componentId: string,
    values?: Record<string, JsonValue>,
  ): Promise<void>;
  /** Changes settings and calls the `onChange` listeners. */
  changeSettings(values: Record<string, JsonValue>): void;
  /** Delivers a new message to the arrival hook; valid actions are applied to `messages`. */
  runArrival(message: PluginMessage | PluginHookMessage): Promise<TestHookRun<PluginArrivalResult>>;
  /** Hands an outgoing message to the before-send hook. */
  runBeforeSend(message: PluginOutgoingMessage): Promise<TestHookRun<PluginBeforeSendResult>>;
  /** Opens a message: runs the open hook. */
  runOpen(message: PluginMessage | PluginHookMessage): Promise<TestHookRun<PluginOpenResult>>;
  /** Presses a banner button. */
  clickBanner(action: PluginBannerAction): Promise<void>;
  /** Runs a composer action on a draft (selection and mode default to empty / `markdown`). */
  runComposerAction(name: string, composer?: Partial<PluginComposerContext>): Promise<void>;
  /** Renders a Markdown block (checking the answer). */
  renderMarkdownBlock(
    name: string,
    source: string,
    purpose?: PluginRenderPurpose,
  ): Promise<PluginMarkdownBlockResult>;
  /** Renders an attachment preview (checking size, the tree or the image). */
  renderPreview(
    name: string,
    attachment: Partial<PluginAttachmentRef> & { name: string },
    bytes: Uint8Array | string,
  ): Promise<PluginPreview>;
  /** Runs an attachment action with the attachment's bytes. */
  runAttachmentAction(
    name: string,
    attachment: Partial<PluginAttachmentRef> & { name: string },
    bytes: Uint8Array | string,
  ): Promise<void>;
  /** Runs a link action. */
  runLinkAction(name: string, link: PluginLink): Promise<void>;
  /** Runs a background job (manual trigger unless given). */
  runJob(id: string, trigger?: PluginJobTrigger): Promise<TestJobRun>;
  /** Calls an MCP tool like an agent would; resolves with its result. */
  callMcpTool(id: string, args?: Record<string, JsonValue>): Promise<JsonValue | undefined>;
}

function commandContext(ctx: Partial<PluginCommandContext> = {}): PluginCommandContext {
  return { surface: "global", ...ctx, selection: ctx.selection ?? { messageIds: [] } };
}

function hostAllowed(pattern: string, host: string): boolean {
  if (pattern.startsWith("*.")) return host.endsWith(`.${pattern.slice(2)}`);
  return host === pattern;
}

/** Scheme and host of a URL (no `URL` global: plug-ins run without Node or DOM). */
function parseUrl(url: string): { protocol: string; hostname: string } {
  const match = /^([a-z][a-z0-9+.-]*:)\/\/(?:[^@/?#]*@)?(\[[^\]]*\]|[^:/?#]*)/i.exec(url);
  if (!match) throw createPluginApiError("invalid_request", `not a URL: ${url}`);
  return { protocol: (match[1] ?? "").toLowerCase(), hostname: (match[2] ?? "").toLowerCase() };
}

/** A JSON copy, as a value that went through Nail would be. */
function clone<T>(value: T): T {
  return JSON.parse(JSON.stringify(value)) as T;
}

/**
 * Runs `fn` like the body of an `async` function: its result, or what it
 * throws, settles the returned promise. The host API is asynchronous; the
 * test host does its work synchronously.
 */
function asPromise<T>(fn: () => T | PromiseLike<T>): Promise<T> {
  return new Promise((resolve) => {
    resolve(fn());
  });
}

function format(args: unknown[]): string {
  return args
    .map((a) => {
      if (typeof a === "string") return a;
      if (a instanceof Error) return `${a.name}: ${a.message}`;
      try {
        // `undefined` for functions, symbols and `undefined` itself.
        const json = JSON.stringify(a) as string | undefined;
        return json ?? String(a);
      } catch {
        return String(a);
      }
    })
    .join(" ");
}

function describeError(error: unknown): string {
  return error instanceof Error ? `${error.name}: ${error.message}` : String(error);
}

const TIMED_OUT = Symbol("timed out");

/** Runs `fn` with a wall-clock budget. */
async function withBudget<T>(ms: number, fn: () => T | Promise<T>): Promise<T | typeof TIMED_OUT> {
  let cancel = () => {
    /* no-op */
  };
  const timeout = new Promise<typeof TIMED_OUT>((resolve) => {
    const id = setTimeout(() => {
      resolve(TIMED_OUT);
    }, ms);
    cancel = () => {
      clearTimeout(id);
    };
  });
  try {
    return await Promise.race([Promise.resolve().then(fn), timeout]);
  } finally {
    cancel();
  }
}

/** Creates an in-memory host. */
export function createTestHost(options: TestHostOptions = {}): TestHost {
  const manifest = options.manifest;
  const contributes = manifest?.contributes;
  const plugin = Object.freeze({
    id: manifest?.id ?? "test-plugin",
    version: manifest?.version ?? "0.0.0",
  });
  const granted = new Set<string>(
    options.permissions ?? manifest?.permissions ?? ALL_FIXED_PERMISSIONS,
  );
  const grantAccounts = options.accounts ? new Set(options.accounts) : undefined;
  const declared = (list: { id: string }[] | undefined) =>
    manifest ? new Set((list ?? []).map((c) => c.id)) : undefined;
  const declaredCommands = declared(contributes?.commands);
  const declaredPanels = declared(contributes?.panels);
  const declaredHooks = manifest
    ? new Set((contributes?.hooks ?? []).map((h) => h.hook))
    : undefined;
  const declaredComposer = declared(contributes?.composerActions);
  const declaredBlocks = declared(contributes?.markdownBlocks);
  const declaredPreviews = declared(contributes?.attachmentPreviews);
  const declaredAttachmentActions = declared(contributes?.attachmentActions);
  const declaredLinkActions = declared(contributes?.linkActions);
  const declaredJobs = declared(contributes?.jobs);
  const declaredTools = declared(contributes?.mcpTools);

  const messages = new Map((options.messages ?? []).map((m) => [m.id, clone(m)]));
  const bodies = options.bodies ?? {};
  let settings: Record<string, JsonValue> = { ...options.settings };
  const settingsListeners: ((values: Record<string, JsonValue>) => void)[] = [];
  const quota = options.storageQuota ?? 5 * 1024 * 1024;
  const dialogs: PluginDialogSpec[] = [];
  const notifications: PluginNotificationSpec[] = [];
  const panelUpdates: { name: string; tree: PluginUiNode[] }[] = [];
  const logs: TestLogLine[] = [];
  const storage = new Map<string, JsonValue>();
  const writes: TestWrite[] = [];
  const confirmations: TestConfirmation[] = [];
  const composerEdits: { draftId: DraftId; edit: ComposerEdit }[] = [];
  const drafts = new Map<string, TestDraft>();
  const tasks = new Map<string, TestTask>();
  const calendarEvents = new Map<string, TestEvent>();
  const contactsById = new Map((options.contacts ?? []).map((c) => [c.id, clone(c)]));
  const calendars = options.calendars ?? [
    { id: "calendar", name: "Calendar", default: true, readOnly: false },
  ];
  const taskLists = options.taskLists ?? [
    { id: "tasks", name: "Tasks", default: true, readOnly: false },
  ];
  const addressBooks = options.addressBooks ?? [
    { id: "contacts", name: "Contacts", default: true, readOnly: false },
  ];
  const l10n = createLocalizer(options.locale ?? "en", options.catalogs ?? {});

  const commands = new Map<string, PluginCommandHandler>();
  const panels = new Map<string, PluginPanelProvider>();
  const hooks = new Map<
    PluginHookKind,
    PluginArrivalHook | PluginBeforeSendHook | PluginOpenHook
  >();
  let bannerHandler: PluginBannerActionHandler | undefined;
  const composerActions = new Map<string, PluginActionHandler>();
  const markdownBlocks = new Map<string, PluginMarkdownBlockRenderer>();
  const previews = new Map<string, PluginPreviewRenderer>();
  const attachmentActions = new Map<string, PluginActionHandler>();
  const linkActions = new Map<string, PluginActionHandler>();
  const jobs = new Map<string, PluginJobHandler>();
  const mcpTools = new Map<string, PluginMcpToolHandler>();

  let counter = 0;
  const nextId = (prefix: string) => `${prefix}-${String(++counter)}`;

  const has = (permission: string, accountId?: string): boolean =>
    granted.has(permission) &&
    (accountId === undefined ||
      !ACCOUNT_PERMISSIONS.has(permission) ||
      grantAccounts === undefined ||
      grantAccounts.has(accountId));
  const need = (permission: string, accountId?: string): void => {
    if (!granted.has(permission)) {
      throw createPluginApiError("forbidden", `permission ${permission} is not granted`);
    }
    if (!has(permission, accountId)) {
      throw createPluginApiError(
        "forbidden",
        `permission ${permission} is not granted for account ${accountId ?? ""}`,
      );
    }
  };
  const checked = (nodes: PluginUiNode[], root: string): void => {
    const issues = checkTree(nodes, root);
    if (issues.length > 0) {
      throw createPluginApiError(
        "invalid_request",
        issues.map((i) => `${i.path}: ${i.message}`).join("; "),
      );
    }
  };
  const used = (): number => {
    let n = 0;
    for (const [k, v] of storage) n += k.length + JSON.stringify(v).length;
    return n;
  };
  const register = <T>(
    kind: string,
    map: Map<string, T>,
    declaredNames: Set<string> | undefined,
    name: string,
    value: T,
  ): void => {
    if (typeof value !== "function" && (typeof value !== "object" || value === null)) {
      throw new TypeError(`a ${kind} handler must be a function`);
    }
    if (declaredNames && !declaredNames.has(name)) {
      throw createPluginApiError("invalid_request", `${kind} ${name} is not in the manifest`);
    }
    if (map.has(name)) throw createPluginApiError("conflict", `${kind} ${name} is registered`);
    map.set(name, value);
  };
  const registerCommand = (name: string, handler: PluginCommandHandler): void => {
    register("command", commands, declaredCommands, name, handler);
  };
  const registerPanel = (name: string, panel: PluginPanelProvider | PluginPanelRender): void => {
    register(
      "panel",
      panels,
      declaredPanels,
      name,
      typeof panel === "function" ? { render: panel } : panel,
    );
  };
  const log =
    (level: PluginLogLevel) =>
    (...args: unknown[]): void => {
      logs.push({ level, message: format(args) });
    };
  const settingsApi: NailApi["settings"] = {
    // The public signature lets callers name the value's type (`get<string>(key)`).
    get: ((key: string) => settings[key]) as NailApi["settings"]["get"],
    all: () => ({ ...settings }),
    onChange: (listener) => {
      settingsListeners.push(listener);
    },
  };

  const policy = (kind: PluginActionKind): "ask" | "allow" => {
    if (DESTRUCTIVE.has(kind)) return "ask";
    return options.policies?.[kind] ?? (ASK_BY_DEFAULT.has(kind) ? "ask" : "allow");
  };

  /** Asks the user when the policy says so, then runs `apply`. */
  const write = async (
    operation: string,
    kind: PluginActionKind,
    input: unknown,
    targets: string[],
    summary: string,
    apply: () => string[],
  ): Promise<PluginWriteResult> => {
    let confirmationId: string | undefined;
    let result: PluginWriteResult | undefined;
    if (policy(kind) === "ask") {
      confirmationId = nextId("confirmation");
      const request = {
        id: confirmationId,
        kind,
        operation,
        summary,
        targets,
        canAlwaysAllow: !DESTRUCTIVE.has(kind),
      };
      const decision = options.confirm ? await options.confirm(request) : "approve";
      confirmations.push({ ...request, decision });
      if (decision === "deny") result = { status: "denied", confirmationId };
      if (decision === "expire") result = { status: "expired", confirmationId };
    }
    if (!result) {
      const created = apply();
      result = { status: "done", opIds: [nextId("op")], created };
      if (confirmationId) result.confirmationId = confirmationId;
    }
    writes.push({ operation, kind, input: clone((input ?? null) as JsonValue), result });
    return result;
  };

  const message = (id: string): PluginMessage => {
    const m = messages.get(id);
    if (!m) throw createPluginApiError("not_found", `message ${id}`);
    return m;
  };
  const collection = (list: PluginCollection[], id: string | undefined, what: string) => {
    const found =
      id === undefined ? (list.find((c) => c.default) ?? list[0]) : list.find((c) => c.id === id);
    if (!found) throw createPluginApiError("not_found", `${what} ${id ?? "(default)"}`);
    if (found.readOnly) throw createPluginApiError("forbidden", `${what} ${found.id} is read-only`);
    return found;
  };
  const invalid = (text: string) => createPluginApiError("invalid_request", text);
  const idList = (ids: string | string[]): string[] => {
    const list = [...new Set(Array.isArray(ids) ? ids : [ids])];
    if (list.length === 0 || list.length > 500) throw invalid("1 to 500 message ids");
    return list;
  };
  let lastComposerDraft: DraftId | undefined;
  const composerEdit = (
    op: ComposerEdit["op"],
    text: string,
    editOptions: { draftId?: DraftId; format?: ComposerEdit["format"] } = {},
  ) =>
    asPromise(() => {
      need("composer");
      if (editOptions.format && !["text", "markdown", "html"].includes(editOptions.format)) {
        throw invalid(`unknown format ${editOptions.format}`);
      }
      const draftId = editOptions.draftId ?? lastComposerDraft;
      if (!draftId || (lastComposerDraft !== undefined && draftId !== lastComposerDraft)) {
        throw createPluginApiError(
          "not_found",
          "no draft to edit: composer edits work only on the draft a composer action ran on",
        );
      }
      const edit: ComposerEdit = { op, text };
      if (editOptions.format) edit.format = editOptions.format;
      composerEdits.push({ draftId, edit });
    });
  const checkPriority = (priority: number | undefined) => {
    if (priority !== undefined && (!Number.isInteger(priority) || priority < 0 || priority > 9)) {
      throw invalid("priority must be 0–9");
    }
  };

  const applyChanges = (m: PluginMessage, c: PluginMessageChanges) => {
    if (c.read !== undefined) m.flags.seen = c.read;
    if (c.flagged !== undefined) m.flags.flagged = c.flagged;
    for (const tag of c.addTags ?? []) if (!m.tags.includes(tag)) m.tags.push(tag);
    if (c.removeTags?.length) m.tags = m.tags.filter((t) => !c.removeTags?.includes(t));
    if (c.moveToFolderId) m.folderId = c.moveToFolderId;
    if (c.archive) m.folderId = "Archive";
  };

  const hooksRegistry: PluginHooksRegistry = {
    register(hook, handler) {
      if (!["arrival", "beforeSend", "open"].includes(hook)) {
        throw createPluginApiError("invalid_request", `unknown hook ${hook}`);
      }
      register(
        "hook",
        hooks as Map<string, unknown>,
        declaredHooks as Set<string> | undefined,
        hook,
        handler,
      );
    },
    onBannerAction(handler) {
      if (typeof handler !== "function") {
        throw new TypeError("a banner handler must be a function");
      }
      if (declaredHooks && !declaredHooks.has("open")) {
        throw createPluginApiError("invalid_request", "banner actions need an open hook");
      }
      bannerHandler = handler;
    },
  };
  const composerRegistry: PluginComposerRegistry = {
    registerAction: (name, handler) => {
      register("composer action", composerActions, declaredComposer, name, handler);
    },
    registerBlock: (name, renderer) => {
      register("Markdown block", markdownBlocks, declaredBlocks, name, renderer);
    },
  };
  const attachmentsRegistry: PluginAttachmentsRegistry = {
    registerPreview: (name, renderer) => {
      register("attachment preview", previews, declaredPreviews, name, renderer);
    },
    registerAction: (name, handler) => {
      register("attachment action", attachmentActions, declaredAttachmentActions, name, handler);
    },
  };
  const linksRegistry: PluginLinksRegistry = {
    registerAction: (name, handler) => {
      register("link action", linkActions, declaredLinkActions, name, handler);
    },
  };
  const jobsRegistry: PluginJobsRegistry = {
    register: (id, handler) => {
      register("job", jobs, declaredJobs, id, handler);
    },
  };
  const mcpRegistry: PluginMcpRegistry = {
    registerTool: (id, handler) => {
      register("MCP tool", mcpTools, declaredTools, id, handler);
    },
  };

  const api: NailApi = {
    plugin,
    apiVersion: "1",
    mail: {
      get: (id) =>
        asPromise(() => {
          need("mail.read");
          const m = message(id);
          need("mail.read", m.accountId);
          return clone(m);
        }),
      getBody: (id) =>
        asPromise(() => {
          need("mail.read");
          need("mail.read.body");
          const m = message(id);
          need("mail.read.body", m.accountId);
          const body = bodies[id];
          if (!body) throw createPluginApiError("not_found", `message ${id}`);
          if (m.encrypted) throw createPluginApiError("forbidden", "end-to-end encrypted mail");
          return clone(body);
        }),
      search: (query, searchOptions = {}) =>
        asPromise(() => {
          need("mail.read");
          // Like Nail: message text is searched only with `mail.read.body`.
          if (/(^|[\s(-])(body|filename):/i.test(query)) need("mail.read.body");
          const words = query.toLowerCase().split(/\s+/).filter(Boolean);
          return [...messages.values()]
            .filter((m) => has("mail.read", m.accountId))
            .filter((m) => words.every((w) => m.subject.toLowerCase().includes(w)))
            .sort((a, b) => b.date.localeCompare(a.date))
            .slice(0, searchOptions.limit ?? 50)
            .map(clone);
        }),
      thread: (threadId) =>
        asPromise(() => {
          need("mail.read");
          return [...messages.values()]
            .filter((m) => m.threadId === threadId && has("mail.read", m.accountId))
            .sort((a, b) => a.date.localeCompare(b.date))
            .map(clone);
        }),
      async update(ids, changes) {
        need("mail.organize");
        const messageIds = idList(ids);
        const targets = messageIds.map(message);
        for (const m of targets) need("mail.organize", m.accountId);
        return write(
          "nail.mail.update",
          "organize",
          { messageIds, changes },
          messageIds,
          `Change ${String(messageIds.length)} message(s)`,
          () => {
            for (const m of targets) applyChanges(m, changes);
            return [];
          },
        );
      },
      async delete(ids, deleteOptions = {}) {
        need("mail.organize");
        const messageIds = idList(ids);
        const targets = messageIds.map(message);
        for (const m of targets) need("mail.organize", m.accountId);
        const permanent = deleteOptions.permanent === true;
        return write(
          "nail.mail.delete",
          permanent ? "permanent-delete" : "trash",
          { messageIds, permanent },
          messageIds,
          `${permanent ? "Delete" : "Move to Trash"} ${String(messageIds.length)} message(s)`,
          () => {
            for (const m of targets) {
              if (permanent) messages.delete(m.id);
              else m.folderId = "Trash";
            }
            return [];
          },
        );
      },
      async createDraft(draft) {
        need("mail.send");
        if (draft.replyToMessageId) need("mail.read", message(draft.replyToMessageId).accountId);
        return write(
          "nail.mail.createDraft",
          "draft",
          draft,
          [],
          `Save a draft “${draft.subject ?? ""}”`,
          () => {
            const id = nextId("draft");
            drafts.set(id, { ...clone(draft), id, sent: false });
            return [id];
          },
        );
      },
      async updateDraft(draftId, changes: PluginDraftChanges) {
        need("mail.send");
        const draft = drafts.get(draftId);
        if (!draft || draft.sent) throw createPluginApiError("not_found", `draft ${draftId}`);
        return write(
          "nail.mail.updateDraft",
          "draft",
          { draftId, changes },
          [draftId],
          `Change the draft “${changes.subject ?? draft.subject ?? ""}”`,
          () => {
            const { to, cc, bcc, subject, body, format: bodyFormat } = clone(changes);
            Object.assign(
              draft,
              Object.fromEntries(
                Object.entries({ to, cc, bcc, subject, body, format: bodyFormat }).filter(
                  ([, v]) => v !== undefined,
                ),
              ),
            );
            return [];
          },
        );
      },
      async send(draftId, sendOptions = {}) {
        need("mail.send");
        const draft = drafts.get(draftId);
        if (!draft) throw createPluginApiError("not_found", `draft ${draftId}`);
        const reasons = options.sendVetoes?.(draft) ?? [];
        if (reasons.length > 0) {
          const result: PluginWriteResult = { status: "vetoed", reasons };
          writes.push({
            operation: "nail.mail.send",
            kind: "send",
            input: clone({ draftId, ...sendOptions }),
            result,
          });
          return result;
        }
        return write(
          "nail.mail.send",
          "send",
          { draftId, ...sendOptions },
          [draftId],
          `Send “${draft.subject ?? ""}” to ${(draft.to ?? []).join(", ")}`,
          () => {
            draft.sent = true;
            return [];
          },
        );
      },
    },
    calendar: {
      events: (from, to, eventOptions = {}) =>
        asPromise(() => {
          need("calendar.read");
          return (options.events ?? [])
            .filter((e) => e.start < to && e.end > from)
            .sort((a, b) => a.start.localeCompare(b.start))
            .slice(0, eventOptions.limit ?? 500)
            .map(clone);
        }),
      calendars: () =>
        asPromise(() => {
          need("calendar.write");
          return clone(calendars);
        }),
      async createEvent(event) {
        need("calendar.write");
        if (!event.title.trim()) throw invalid("an event needs a title");
        if (!event.start || !event.end || event.end < event.start) {
          throw invalid("an event needs a start before its end");
        }
        const invites = event.sendInvitations === true && (event.attendees ?? []).length > 0;
        if (invites) need("mail.send");
        const target = collection(calendars, event.calendarId, "calendar");
        return write(
          "nail.calendar.createEvent",
          invites ? "invite" : "calendar",
          event,
          [],
          `Create the event “${event.title}”`,
          () => {
            const id = nextId("event");
            calendarEvents.set(id, { ...clone(event), id, calendarId: target.id });
            return [id];
          },
        );
      },
      async updateEvent(eventId, changes) {
        need("calendar.write");
        const event = calendarEvents.get(eventId);
        if (!event) throw createPluginApiError("not_found", `event ${eventId}`);
        if (changes.sendUpdates) need("mail.send");
        return write(
          "nail.calendar.updateEvent",
          changes.sendUpdates ? "invite" : "calendar",
          { eventId, changes },
          [eventId],
          `Change the event “${event.title}”`,
          () => {
            const { title, start, end, location, description } = changes;
            Object.assign(event, clone({ title, start, end, location, description }));
            return [];
          },
        );
      },
      async deleteEvent(eventId) {
        need("calendar.write");
        const event = calendarEvents.get(eventId);
        if (!event) throw createPluginApiError("not_found", `event ${eventId}`);
        return write(
          "nail.calendar.deleteEvent",
          "pim-delete",
          { eventId },
          [eventId],
          `Delete the event “${event.title}”`,
          () => {
            calendarEvents.delete(eventId);
            return [];
          },
        );
      },
      async respond(answer) {
        need("calendar.write");
        if ((answer.messageId === undefined) === (answer.eventId === undefined)) {
          throw invalid("pass either messageId or eventId");
        }
        if (!["accept", "tentative", "decline"].includes(answer.response)) {
          throw invalid(`unknown response ${answer.response}`);
        }
        if (answer.messageId !== undefined) need("mail.read", message(answer.messageId).accountId);
        else if (!calendarEvents.has(answer.eventId ?? "")) {
          throw createPluginApiError("not_found", `event ${answer.eventId ?? "(none)"}`);
        }
        const tell = answer.send !== false;
        if (tell) need("mail.send");
        return write(
          "nail.calendar.respond",
          tell ? "invite" : "calendar",
          answer,
          [answer.messageId ?? answer.eventId ?? ""],
          `Answer “${answer.response}”`,
          () => [],
        );
      },
    },
    contacts: {
      addressBooks: () =>
        asPromise(() => {
          need("contacts.write");
          return clone(addressBooks);
        }),
      find: (email, findOptions = {}) =>
        asPromise(() => {
          need("contacts.write");
          const lower = email.toLowerCase();
          return [...contactsById.values()]
            .filter((c) => c.emails.some((e) => e.toLowerCase() === lower))
            .slice(0, findOptions.limit ?? 10)
            .map(clone);
        }),
      async upsert(input: PluginContactInput) {
        need("contacts.write");
        const existing = input.contactId ? contactsById.get(input.contactId) : undefined;
        if (input.contactId && !existing) {
          throw createPluginApiError("not_found", `contact ${input.contactId}`);
        }
        const book = existing
          ? undefined
          : collection(addressBooks, input.addressBookId, "address book");
        return write(
          "nail.contacts.upsert",
          "contact",
          input,
          input.contactId ? [input.contactId] : [],
          `${existing ? "Change" : "Create"} the contact “${input.fullName ?? existing?.fullName ?? ""}”`,
          () => {
            const id = existing?.id ?? nextId("contact");
            const next: PluginContact = existing ?? {
              id,
              addressBookId: book?.id ?? "contacts",
              emails: [],
            };
            if (input.fullName !== undefined) next.fullName = input.fullName;
            if (input.emails) next.emails = [...input.emails];
            if (input.organization !== undefined) next.organization = input.organization;
            contactsById.set(id, next);
            return existing ? [] : [id];
          },
        );
      },
      async delete(contactId) {
        need("contacts.write");
        const existing = contactsById.get(contactId);
        if (!existing) throw createPluginApiError("not_found", `contact ${contactId}`);
        return write(
          "nail.contacts.delete",
          "pim-delete",
          { contactId },
          [contactId],
          `Delete the contact “${existing.fullName ?? existing.emails[0] ?? ""}”`,
          () => {
            contactsById.delete(contactId);
            return [];
          },
        );
      },
    },
    tasks: {
      lists: () =>
        asPromise(() => {
          need("tasks.write");
          return clone(taskLists);
        }),
      async create(task) {
        need("tasks.write");
        if (typeof task.title !== "string" || !task.title.trim())
          throw invalid("a task needs a title");
        checkPriority(task.priority);
        if (task.list !== undefined && task.listId !== undefined) {
          throw invalid("pass either list or listId");
        }
        let related = task.relatedMessageId;
        for (const link of task.links ?? []) {
          const id = /^nail:\/\/messages\/([^/]+)$/.exec(link)?.[1];
          if (!id) throw invalid(`not a nail://messages/ link: ${link}`);
          if (related !== undefined && related !== id) {
            throw invalid("a task links to at most one message");
          }
          related = id;
        }
        if (related !== undefined) need("mail.read", message(related).accountId);
        const list = collection(taskLists, task.list ?? task.listId, "task list");
        return write(
          "nail.tasks.create",
          "task",
          task,
          [],
          `Create the task “${task.title}”`,
          () => {
            const id = nextId("task");
            tasks.set(id, {
              ...clone(task),
              id,
              list: list.id,
              completed: task.completed === true,
            });
            return [id];
          },
        );
      },
      async update(taskId, changes) {
        need("tasks.write");
        const task = tasks.get(taskId);
        if (!task) throw createPluginApiError("not_found", `task ${taskId}`);
        checkPriority(changes.priority);
        return write(
          "nail.tasks.update",
          "task",
          { taskId, changes },
          [taskId],
          `Change the task “${task.title}”`,
          () => {
            if (changes.title !== undefined) task.title = changes.title;
            if (changes.due === "") delete task.due;
            else if (changes.due !== undefined) task.due = changes.due;
            if (changes.notes !== undefined) task.notes = changes.notes;
            if (changes.priority !== undefined) task.priority = changes.priority;
            if (changes.completed !== undefined) task.completed = changes.completed;
            return [];
          },
        );
      },
      async delete(taskId) {
        need("tasks.write");
        const task = tasks.get(taskId);
        if (!task) throw createPluginApiError("not_found", `task ${taskId}`);
        return write(
          "nail.tasks.delete",
          "pim-delete",
          { taskId },
          [taskId],
          `Delete the task “${task.title}”`,
          () => {
            tasks.delete(taskId);
            return [];
          },
        );
      },
    },
    composer: {
      ...composerRegistry,
      insert: (text, editOptions) => composerEdit("insert", text, editOptions),
      replaceSelection: (text, editOptions) => composerEdit("replace-selection", text, editOptions),
      replaceAll: (text, editOptions) => composerEdit("replace-all", text, editOptions),
    },
    hooks: hooksRegistry,
    attachments: attachmentsRegistry,
    links: linksRegistry,
    jobs: jobsRegistry,
    mcp: mcpRegistry,
    ui: {
      registerCommand,
      registerPanel,
      async showDialog(spec) {
        need("ui.dialogs");
        checked(spec.body, "body");
        dialogs.push(spec);
        return options.dialog ? options.dialog(spec) : { button: null, values: {} };
      },
      notify: (notification) =>
        asPromise(() => {
          need("notifications");
          const spec = typeof notification === "string" ? { title: notification } : notification;
          if (!spec.title.trim() || spec.title.length > 200 || (spec.body?.length ?? 0) > 2000) {
            throw createPluginApiError(
              "invalid_request",
              "the notification title or body is empty or too long",
            );
          }
          notifications.push(spec);
        }),
      updatePanel: (name, tree) =>
        asPromise(() => {
          if (!panels.has(name)) throw createPluginApiError("not_found", `panel ${name}`);
          checked(tree, name);
          panelUpdates.push({ name, tree });
        }),
    },
    commands: { register: registerCommand },
    storage: {
      get: <T extends JsonValue = JsonValue>(key: string) =>
        asPromise(() => {
          need("storage");
          const value = storage.get(key);
          return (value === undefined ? null : clone(value)) as T | null;
        }),
      set: (key, value) =>
        asPromise(() => {
          need("storage");
          if (key.length === 0 || key.length > 256) {
            throw createPluginApiError("invalid_request", "storage keys are 1–256 bytes");
          }
          const before = storage.get(key);
          storage.set(key, clone(value));
          if (used() > quota) {
            if (before === undefined) storage.delete(key);
            else storage.set(key, before);
            throw createPluginApiError("too_large", "storage quota exceeded");
          }
        }),
      delete: (key) =>
        asPromise(() => {
          need("storage");
          return storage.delete(key);
        }),
      keys: (prefix) =>
        asPromise(() => {
          need("storage");
          return [...storage.keys()].filter((k) => !prefix || k.startsWith(prefix)).sort();
        }),
      clear: () =>
        asPromise(() => {
          need("storage");
          storage.clear();
        }),
      usage: (): Promise<PluginStorageUsage> =>
        asPromise(() => {
          need("storage");
          return { keys: storage.size, usedBytes: used(), quotaBytes: quota };
        }),
    },
    http: {
      async fetch(url, request = {}) {
        const parsed = parseUrl(url);
        const allowed = [...granted].some(
          (p) => p.startsWith("network:") && hostAllowed(p.slice(8), parsed.hostname),
        );
        if (!allowed) {
          throw createPluginApiError("forbidden", `permission network:${parsed.hostname}`);
        }
        if (parsed.protocol !== "https:") {
          throw createPluginApiError("invalid_request", `scheme ${parsed.protocol} is not allowed`);
        }
        const method = request.method ?? "GET";
        if ((method === "GET" || method === "HEAD") && request.body !== undefined) {
          throw createPluginApiError("invalid_request", `${method} takes no body`);
        }
        const res = options.fetch ? await options.fetch(url, request) : { status: 404 };
        const status = res.status ?? 200;
        const raw = res.body ?? "";
        const bytes = typeof raw === "string" ? encodeUtf8(raw) : toBytes(raw);
        const body = typeof raw === "string" ? raw : decodeUtf8(bytes);
        return {
          status,
          ok: status >= 200 && status < 300,
          headers: res.headers ?? {},
          body,
          text: () => asPromise(() => body),
          json: <T>() => asPromise(() => JSON.parse(body) as T),
          bytes: () => asPromise(() => bytes.slice()),
          arrayBuffer: () => asPromise(() => bytes.slice().buffer),
        };
      },
    },
    settings: settingsApi,
    l10n,
    log: {
      debug: log("debug"),
      info: log("info"),
      warn: log("warn"),
      error: log("error"),
    },
  };

  const activation: PluginActivationContext = {
    plugin,
    commands: { register: registerCommand },
    panels: { register: registerPanel },
    settings: settingsApi,
    hooks: hooksRegistry,
    composer: composerRegistry,
    attachments: attachmentsRegistry,
    links: linksRegistry,
    jobs: jobsRegistry,
    mcp: mcpRegistry,
    l10n,
  };

  const budget = (hook: PluginHookKind): number =>
    options.hookTimeoutMs?.[hook] ??
    contributes?.hooks?.find((h) => h.hook === hook)?.timeoutMs ??
    HOOK_TIMEOUT_MS[hook];

  const hookMessage = (mail: PluginMessage | PluginHookMessage): PluginHookMessage => {
    const m = "message" in mail ? mail.message : mail;
    const out: PluginHookMessage = { message: clone(m) };
    const body = "message" in mail && mail.body ? mail.body : bodies[m.id];
    if (body && !m.encrypted && has("mail.read.body", m.accountId)) out.body = clone(body);
    return out;
  };

  async function runHook<T>(
    hook: PluginHookKind,
    accountId: string,
    input: unknown,
    problemsOf: (result: T | null | undefined) => string[],
    extra?: (result: T) => string[],
  ): Promise<TestHookRun<T>> {
    const handler = hooks.get(hook) as ((input: unknown) => unknown) | undefined;
    const permission = `hooks.${hook}`;
    if (!handler) return { outcome: "skipped", problems: [], error: `no ${hook} hook registered` };
    if (!has(permission, accountId) || (hook !== "beforeSend" && !has("mail.read", accountId))) {
      return {
        outcome: "skipped",
        problems: [],
        error: `${permission} is not granted for ${accountId}`,
      };
    }
    let answer: unknown;
    try {
      answer = await withBudget(budget(hook), () => handler(clone(input)));
    } catch (error) {
      return { outcome: "failed", problems: [], error: describeError(error) };
    }
    if (answer === TIMED_OUT) {
      return {
        outcome: "timed-out",
        problems: [],
        error: `over its ${String(budget(hook))} ms budget`,
      };
    }
    const result = (answer ?? undefined) as T | undefined;
    if (result !== undefined && (typeof result !== "object" || Array.isArray(result))) {
      return { outcome: "failed", problems: ["not an object"] };
    }
    const problems = [...problemsOf(result), ...(result && extra ? extra(result) : [])];
    if (problems.length > 0) return { outcome: "failed", problems };
    const vetoed =
      hook === "beforeSend" && Boolean((result as PluginBeforeSendResult | undefined)?.veto);
    const run: TestHookRun<T> = { outcome: vetoed ? "vetoed" : "ok", problems: [] };
    if (result !== undefined) run.result = clone(result);
    return run;
  }

  const attachmentRef = (
    attachment: Partial<PluginAttachmentRef> & { name: string },
    bytes: Uint8Array,
  ): PluginAttachmentRef => ({
    messageId: attachment.messageId ?? "message",
    index: attachment.index ?? 0,
    name: attachment.name,
    mimeType: attachment.mimeType ?? "application/octet-stream",
    size: attachment.size ?? bytes.length,
  });
  const asBytes = (bytes: Uint8Array | string) =>
    typeof bytes === "string" ? encodeUtf8(bytes) : bytes;
  const attachmentAccount = (ref: PluginAttachmentRef) => messages.get(ref.messageId)?.accountId;

  return {
    api,
    dialogs,
    notifications,
    panelUpdates,
    logs,
    storage,
    writes,
    confirmations,
    composerEdits,
    messages,
    drafts,
    tasks,
    calendarEvents,
    contactsById,
    commandNames: () => [...commands.keys()],
    panelNames: () => [...panels.keys()],
    hookNames: () => [...hooks.keys()],
    extensionNames: () => ({
      composerActions: [...composerActions.keys()],
      markdownBlocks: [...markdownBlocks.keys()],
      attachmentPreviews: [...previews.keys()],
      attachmentActions: [...attachmentActions.keys()],
      linkActions: [...linkActions.keys()],
      jobs: [...jobs.keys()],
      mcpTools: [...mcpTools.keys()],
    }),
    install() {
      const g = globalThis as { nail?: NailApi };
      const previous = g.nail;
      g.nail = api;
      return () => {
        if (previous === undefined) delete g.nail;
        else g.nail = previous;
      };
    },
    activationContext: () => activation,
    async runCommand(name, ctx) {
      const handler = commands.get(name);
      if (!handler) throw createPluginApiError("not_found", `command ${name}`);
      await handler(commandContext(ctx));
    },
    async renderPanel(name, ctx) {
      const provider = panels.get(name);
      if (!provider) throw createPluginApiError("not_found", `panel ${name}`);
      const tree = await provider.render(commandContext(ctx));
      checked(tree, name);
      return tree;
    },
    async clickPanelButton(name, componentId, values = {}) {
      const provider = panels.get(name);
      if (!provider) throw createPluginApiError("not_found", `panel ${name}`);
      await provider.onAction?.({ componentId, values });
    },
    changeSettings(values) {
      settings = { ...settings, ...values };
      for (const listener of settingsListeners) listener({ ...settings });
    },
    async runArrival(mail) {
      const input = hookMessage(mail);
      const run = await runHook<PluginArrivalResult>(
        "arrival",
        input.message.accountId,
        input,
        arrivalProblems,
        (result) =>
          (result.actions ?? []).length > 0 && !has("mail.organize", input.message.accountId)
            ? ["actions need mail.organize"]
            : [],
      );
      const actions = run.result?.actions ?? [];
      const target = messages.get(input.message.id);
      if (run.outcome === "ok" && actions.length > 0 && target) {
        const result: PluginWriteResult = { status: "done", opIds: [nextId("op")], created: [] };
        for (const action of actions) {
          switch (action.type) {
            case "addTag":
              applyChanges(target, { addTags: [action.tag] });
              break;
            case "removeTag":
              applyChanges(target, { removeTags: [action.tag] });
              break;
            case "setFlagged":
              applyChanges(target, { flagged: action.flagged });
              break;
            case "setRead":
              applyChanges(target, { read: action.read });
              break;
            case "move":
              applyChanges(target, { moveToFolderId: action.folderId });
              break;
            case "archive":
              applyChanges(target, { archive: true });
              break;
          }
        }
        writes.push({
          operation: "hook.arrival",
          kind: "organize",
          input: clone({ messageId: target.id, actions }),
          result,
        });
      }
      return run;
    },
    runBeforeSend(outgoing) {
      return runHook<PluginBeforeSendResult>(
        "beforeSend",
        outgoing.accountId,
        outgoing,
        beforeSendProblems,
      );
    },
    runOpen(mail) {
      const input = hookMessage(mail);
      return runHook<PluginOpenResult>("open", input.message.accountId, input, openProblems);
    },
    async clickBanner(action) {
      if (!bannerHandler) throw createPluginApiError("not_found", "no banner action handler");
      await bannerHandler(clone(action));
    },
    async runComposerAction(name, composer = {}) {
      const handler = composerActions.get(name);
      if (!handler) throw createPluginApiError("not_found", `composer action ${name}`);
      need("composer");
      const context: PluginComposerContext = {
        draftId: composer.draftId ?? "draft",
        selection: composer.selection ?? { text: "", start: 0, end: 0 },
        mode: composer.mode ?? "markdown",
      };
      lastComposerDraft = context.draftId;
      await handler({ action: name, composer: context });
    },
    async renderMarkdownBlock(name, source, purpose = "preview") {
      const renderer = markdownBlocks.get(name);
      if (!renderer) throw createPluginApiError("not_found", `Markdown block ${name}`);
      need("composer");
      const result = await renderer({ block: name, source, purpose });
      const problems = markdownBlockProblems(result);
      if (problems.length > 0) throw createPluginApiError("invalid_request", problems.join("; "));
      return clone(result);
    },
    async renderPreview(name, attachment, raw) {
      const renderer = previews.get(name);
      if (!renderer) throw createPluginApiError("not_found", `attachment preview ${name}`);
      const bytes = asBytes(raw);
      const ref = attachmentRef(attachment, bytes);
      need("attachments", attachmentAccount(ref));
      const max = Math.min(
        contributes?.attachmentPreviews?.find((p) => p.id === name)?.maxBytes ?? MAX_PREVIEW_INPUT,
        MAX_PREVIEW_INPUT,
      );
      if (bytes.length > max) throw createPluginApiError("too_large", `over ${String(max)} bytes`);
      const preview = await renderer(ref, bytes.slice());
      const problems = previewProblems(preview);
      if (problems.length > 0) throw createPluginApiError("invalid_request", problems.join("; "));
      return clone(preview);
    },
    async runAttachmentAction(name, attachment, raw) {
      const handler = attachmentActions.get(name);
      if (!handler) throw createPluginApiError("not_found", `attachment action ${name}`);
      const bytes = asBytes(raw);
      const ref = attachmentRef(attachment, bytes);
      need("attachments", attachmentAccount(ref));
      await handler({ action: name, attachment: ref }, bytes.slice());
    },
    async runLinkAction(name, link) {
      const handler = linkActions.get(name);
      if (!handler) throw createPluginApiError("not_found", `link action ${name}`);
      await handler({ action: name, link: clone(link) });
    },
    async runJob(id, trigger = { kind: "manual" }) {
      const handler = jobs.get(id);
      if (!handler) throw createPluginApiError("not_found", `job ${id}`);
      need("background");
      const limit = options.jobTimeoutMs ?? JOB_WALL_CLOCK_MS;
      try {
        const done = await withBudget(limit, () => handler(clone(trigger)));
        if (done === TIMED_OUT) return { outcome: "timeout", error: `over ${String(limit)} ms` };
        return { outcome: "ok" };
      } catch (error) {
        return { outcome: "error", error: describeError(error) };
      }
    },
    async callMcpTool(id, args = {}) {
      const handler = mcpTools.get(id);
      if (!handler) throw createPluginApiError("not_found", `MCP tool ${id}`);
      need("mcp.expose");
      const result = await handler(clone(args));
      return result === undefined ? undefined : clone(result);
    },
  };
}

export { createLocalizer, FluentCatalog, type L10nArgs, type Localizer } from "./l10n";
