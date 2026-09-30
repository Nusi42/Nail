/**
 * The mock core's answers to the M2c junk, snooze and notification
 * requests (ADR 0043), used by `MockTransport` (`../../api/mock.ts` routes
 * exactly these requests here). State lives in `data.features.triage`.
 *
 * Behaves like the core once WP55 wires `nail-triage` (ADR 0043):
 * settings and status are served and validated, `TestNotification` pushes
 * a test `NotificationRequested`; `MarkJunk` trains (per message, so
 * re-marking moves it between the classes), sets `$Junk` / `$NotJunk` and
 * moves the mail into or out of Junk; `SnoozeMessages` sets `$Snoozed` and
 * moves the mail into the hidden Snoozed folder (it leaves every list)
 * until `UnsnoozeMessages` (or `wakeDue`) brings it back. All three are
 * queued writes (`Queued` + `OpStatus`). `decorateJunk` adds the verdict
 * of marked mail to `GetMessage`.
 */
import type {
  FolderId,
  JunkSettings,
  JunkStatus,
  MessageBody,
  MessageSummary,
  NotificationSettings,
  Request,
  Response,
  SnoozedMessage,
} from "../../api/generated";
import { errorResponse, findMessages, queue, type MockCore } from "../../api/mockMail";
import {
  defaultJunkSettings,
  defaultNotificationSettings,
  junkIssues,
  notificationIssues,
} from "./model";

/** The requests this feature answers. */
export const TRIAGE_REQUESTS = [
  "MarkJunk",
  "GetJunkStatus",
  "UpdateJunkSettings",
  "ResetJunkClassifier",
  "SnoozeMessages",
  "UnsnoozeMessages",
  "ListSnoozed",
  "GetNotificationSettings",
  "UpdateNotificationSettings",
  "TestNotification",
] as const satisfies readonly Request["type"][];

export type TriageRequest = Extract<Request, { type: (typeof TRIAGE_REQUESTS)[number] }>;

export function isTriageRequest(request: Request): request is TriageRequest {
  return (TRIAGE_REQUESTS as readonly string[]).includes(request.type);
}

/** Mock-core state of this feature. */
export interface TriageMockState {
  junk: Record<string, JunkSettings>;
  notifications: NotificationSettings;
  snoozed: SnoozedMessage[];
  /** Training per message id: its account and class. */
  training: Record<string, { account: string; junk: boolean }>;
  /** Last training per account (RFC 3339). */
  trainedAt: Record<string, string>;
}

export function createTriageMockState(): TriageMockState {
  return {
    junk: {},
    notifications: defaultNotificationSettings(),
    snoozed: [],
    training: {},
    trainedAt: {},
  };
}

/** The hidden folder snoozed mail waits in (not listed, like the core's). */
export function snoozedFolderId(accountId: string): FolderId {
  return `${accountId}/Snoozed`;
}

const JUNK_KEYWORD = "$Junk";
const NOT_JUNK_KEYWORD = "$NotJunk";
const SNOOZED_KEYWORD = "$Snoozed";

function withTags(m: MessageSummary, add: string[], remove: string[]): string[] {
  return [...new Set([...m.tags.filter((t) => !remove.includes(t)), ...add])].sort();
}

function state(core: MockCore): TriageMockState {
  return core.data.features.triage;
}

function junkStatus(core: MockCore, accountId: string): Response {
  if (!core.data.accountsConfig.accounts.some((a) => a.id === accountId)) {
    return errorResponse("not_found", `account ${accountId}`);
  }
  const s = state(core);
  const settings = s.junk[accountId] ?? defaultJunkSettings();
  const trained = Object.values(s.training).filter((e) => e.account === accountId);
  const junk = trained.filter((e) => e.junk).length;
  const ham = trained.length - junk;
  const status: JunkStatus = {
    account_id: accountId,
    settings: structuredClone(settings),
    trained_junk: junk,
    trained_ham: ham,
    tokens: trained.length * 40,
    ready: junk >= settings.min_training && ham >= settings.min_training,
    last_trained_at: s.trainedAt[accountId] ?? null,
  };
  return { type: "JunkStatus", status };
}

function invalid(what: string, id: string): Response {
  return {
    type: "Error",
    code: "invalid_request",
    message: `invalid ${what} (${id})`,
    localized: { id },
  };
}

export function answerTriage(core: MockCore, request: TriageRequest): Response {
  const s = state(core);
  switch (request.type) {
    case "MarkJunk":
      return markJunk(core, request.message_ids, request.junk);
    case "SnoozeMessages":
      return snooze(core, request.message_ids, request.until, request.wake_folder_id);
    case "UnsnoozeMessages":
      return unsnooze(core, request.message_ids);
    case "GetJunkStatus":
      return junkStatus(core, request.account_id);
    case "UpdateJunkSettings": {
      const issue = junkIssues(request.settings)[0];
      if (issue !== undefined) return invalid("junk settings", `junk-issue-${issue}`);
      const answer = junkStatus(core, request.account_id);
      if (answer.type === "Error") return answer;
      s.junk[request.account_id] = structuredClone(request.settings);
      core.emit({ type: "JunkStatusChanged", account_id: request.account_id });
      return junkStatus(core, request.account_id);
    }
    case "ResetJunkClassifier": {
      const answer = junkStatus(core, request.account_id);
      if (answer.type === "Error") return answer;
      s.training = Object.fromEntries(
        Object.entries(s.training).filter(([, e]) => e.account !== request.account_id),
      );
      s.trainedAt = Object.fromEntries(
        Object.entries(s.trainedAt).filter(([account]) => account !== request.account_id),
      );
      core.emit({ type: "JunkStatusChanged", account_id: request.account_id });
      return junkStatus(core, request.account_id);
    }
    case "ListSnoozed": {
      const account = request.account_id;
      const messages = s.snoozed.filter(
        (m) => account === null || m.message.account_id === account,
      );
      return { type: "Snoozed", messages: structuredClone(messages) };
    }
    case "GetNotificationSettings":
      return { type: "NotificationSettings", settings: structuredClone(s.notifications) };
    case "UpdateNotificationSettings": {
      const issue = notificationIssues(request.settings)[0];
      if (issue !== undefined) {
        return invalid("notification settings", `notification-issue-${issue}`);
      }
      s.notifications = structuredClone(request.settings);
      core.emit({ type: "NotificationSettingsChanged" });
      return { type: "NotificationSettings", settings: structuredClone(s.notifications) };
    }
    case "TestNotification": {
      const sound = s.notifications.sound;
      queueMicrotask(() =>
        core.emit({
          type: "NotificationRequested",
          notification: {
            id: core.nextId("test-"),
            tag: "test",
            kind: "test",
            account_id: null,
            folder_id: null,
            message_ids: [],
            title: { id: "notification-test-title" },
            body: { id: "notification-test-body" },
            sound,
          },
        }),
      );
      return { type: "Done" };
    }
  }
}

function markJunk(core: MockCore, ids: string[], junk: boolean): Response {
  const d = core.data;
  const s = state(core);
  const targets = findMessages(d, ids);
  if (!targets) return errorResponse("not_found", "message not found");
  const now = new Date().toISOString();
  const answer = queue(core, targets, () => {
    const changed = new Set<FolderId>();
    for (const m of targets) {
      s.training[m.id] = { account: m.account_id, junk };
      s.trainedAt[m.account_id] = now;
      m.tags = junk
        ? withTags(m, [JUNK_KEYWORD], [NOT_JUNK_KEYWORD])
        : withTags(m, [NOT_JUNK_KEYWORD], [JUNK_KEYWORD]);
      const folder = d.folders.find((f) => f.id === m.folder_id);
      const move = junk ? folder?.role !== "junk" : folder?.role === "junk";
      const role = junk ? "junk" : "inbox";
      const target = d.folders.find((f) => f.account_id === m.account_id && f.role === role);
      changed.add(m.folder_id);
      if (move && target) {
        m.folder_id = target.id;
        changed.add(target.id);
      }
    }
    return [...changed];
  });
  for (const account of new Set(targets.map((m) => m.account_id))) {
    core.emit({ type: "JunkStatusChanged", account_id: account });
  }
  return answer;
}

function snooze(core: MockCore, ids: string[], until: string, wake: FolderId | null): Response {
  const d = core.data;
  const s = state(core);
  const when = Date.parse(until);
  if (Number.isNaN(when)) return errorResponse("invalid_request", `invalid time ${until}`);
  if (when <= Date.now()) return errorResponse("invalid_request", "the wake time has passed");
  const targets = findMessages(d, ids);
  if (!targets) return errorResponse("not_found", "message not found");
  const wakeFolder = wake === null ? null : d.folders.find((f) => f.id === wake);
  if (wake !== null && !wakeFolder) return errorResponse("not_found", `folder ${wake}`);
  const now = new Date().toISOString();
  const answer = queue(core, targets, () => {
    const changed = new Set<FolderId>();
    for (const m of targets) {
      changed.add(m.folder_id);
      const back =
        wakeFolder && wakeFolder.account_id === m.account_id ? wakeFolder.id : m.folder_id;
      s.snoozed = s.snoozed.filter((e) => e.message.id !== m.id);
      m.tags = withTags(m, [SNOOZED_KEYWORD], []);
      m.folder_id = snoozedFolderId(m.account_id);
      s.snoozed.push({ message: structuredClone(m), until, wake_folder_id: back, snoozed_at: now });
    }
    return [...changed];
  });
  core.emit({ type: "SnoozeChanged" });
  return answer;
}

function unsnooze(core: MockCore, ids: string[]): Response {
  const d = core.data;
  const s = state(core);
  const entries = s.snoozed.filter((e) => ids.includes(e.message.id));
  if (entries.length !== ids.length) return errorResponse("not_found", "message is not snoozed");
  const targets = findMessages(d, ids);
  if (!targets) return errorResponse("not_found", "message not found");
  const answer = queue(core, targets, () => {
    const changed = new Set<FolderId>();
    for (const entry of entries) {
      const m = targets.find((t) => t.id === entry.message.id);
      if (!m) continue;
      m.folder_id = entry.wake_folder_id;
      m.tags = withTags(m, [], [SNOOZED_KEYWORD]);
      changed.add(entry.wake_folder_id);
    }
    s.snoozed = s.snoozed.filter((e) => !ids.includes(e.message.id));
    return [...changed];
  });
  core.emit({ type: "SnoozeChanged" });
  return answer;
}

/** Wakes every snoozed message whose time has come (the core's wake timer). */
export function wakeDue(core: MockCore, now = Date.now()): string[] {
  const due = state(core)
    .snoozed.filter((e) => Date.parse(e.until) <= now)
    .map((e) => e.message.id);
  if (due.length > 0) unsnooze(core, due);
  return due;
}

/** `GetMessage`: the junk verdict of mail marked by the user or flagged by the server. */
export function decorateJunk(core: MockCore, body: MessageBody): MessageBody {
  const training = state(core).training[body.id];
  if (training) return { ...body, junk: { junk: training.junk, score: null, source: "user" } };
  const summary = core.data.messages.find((m) => m.id === body.id);
  if (summary?.tags.includes(JUNK_KEYWORD)) {
    return { ...body, junk: { junk: true, score: null, source: "server" } };
  }
  return body;
}
