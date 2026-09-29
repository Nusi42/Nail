/**
 * The mock core's answers to the M2c junk, snooze and notification
 * requests (ADR 0043), used by `MockTransport` (`../../api/mock.ts` routes
 * exactly these requests here). State lives in `data.features.triage`.
 *
 * Behaves like the core after WP50: settings and status are served and
 * validated, `TestNotification` pushes a test `NotificationRequested`;
 * training, snoozing and waking (`MarkJunk`, `SnoozeMessages`,
 * `UnsnoozeMessages`) answer `unsupported` until WP55 wires `nail-triage`.
 * WP54 may extend it.
 */
import type {
  JunkSettings,
  JunkStatus,
  NotificationSettings,
  Request,
  Response,
  SnoozedMessage,
} from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";
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
}

export function createTriageMockState(): TriageMockState {
  return { junk: {}, notifications: defaultNotificationSettings(), snoozed: [] };
}

function state(core: MockCore): TriageMockState {
  return core.data.features.triage;
}

function junkStatus(core: MockCore, accountId: string): Response {
  if (!core.data.accountsConfig.accounts.some((a) => a.id === accountId)) {
    return errorResponse("not_found", `account ${accountId}`);
  }
  const settings = state(core).junk[accountId] ?? defaultJunkSettings();
  const status: JunkStatus = {
    account_id: accountId,
    settings: structuredClone(settings),
    trained_junk: 0,
    trained_ham: 0,
    tokens: 0,
    ready: false,
    last_trained_at: null,
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
      return errorResponse("unsupported", "junk training is not wired yet");
    case "SnoozeMessages":
    case "UnsnoozeMessages":
      return errorResponse("unsupported", "snoozing is not wired yet");
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
      if (answer.type !== "Error") {
        core.emit({ type: "JunkStatusChanged", account_id: request.account_id });
      }
      return answer;
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
