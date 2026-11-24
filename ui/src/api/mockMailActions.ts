/**
 * Mail actions on conversations in the mock core (P3, ADR 0164):
 * `ApplyMailAction`, `ResolveMailTargets` and `UndoMailAction` with the
 * contract's semantics — targets resolve per action
 * (`nail_api::mail_actions::MailAction::reach`, ported as `reachOf` and
 * pinned by `crates/nail-api/tests/integration/ts_vectors.rs`), the
 * change is applied at once and replayed per account like every mock write,
 * and each action is one batch with one undo (the messages' folders, flags
 * and tags as they were). Snooze and junk reuse the triage mock.
 */
import type {
  ConversationReach,
  FolderId,
  MailAction,
  MailActionIssue,
  MailActionKind,
  MailActionResult,
  MailBatchId,
  MailList,
  ListTarget,
  MessageId,
  MessageSummary,
  MessageTarget,
  MoveTarget,
  Request,
  Response,
} from "./generated";
import {
  deleteMessages,
  errorResponse,
  folderMessages,
  getConversation,
  listThreads,
  moveMessages,
  queue,
  resolveFolder,
  search,
  updateFlags,
  type MockCore,
} from "./mockMail";
import {
  MAIL_ACTION_ISSUE_MESSAGE_IDS,
  MAIL_UNDO_SECS,
  MAX_MAIL_ACTION_TARGETS,
} from "./generated/constants";
import type { MockData } from "./mockData";
import { listRowTarget, type RowTarget } from "../actions/rowTargets";
import { answerTriage } from "../features/triage/mock";

/** The requests answered here. */
export const MAIL_ACTION_REQUESTS = [
  "ApplyMailAction",
  "ResolveMailTargets",
  "UndoMailAction",
] as const satisfies readonly Request["type"][];

export type MailActionRequest = Extract<Request, { type: (typeof MAIL_ACTION_REQUESTS)[number] }>;

export function isMailActionRequest(request: Request): request is MailActionRequest {
  return (MAIL_ACTION_REQUESTS as readonly string[]).includes(request.type);
}

/** `MailAction::reach`. */
export function reachOf(action: MailAction): ConversationReach {
  switch (action.type) {
    case "archive":
    case "move":
    case "snooze":
    case "junk":
    case "delete-permanently":
      return "listed";
    case "trash":
      return "thread";
    case "mark-read":
    case "tags":
      return "whole";
    case "flag":
      return action.flagged ? "newest" : "whole";
  }
}

/** `MailAction::kind`. */
export function kindOf(action: MailAction): MailActionKind {
  switch (action.type) {
    case "mark-read":
      return action.read ? "mark-read" : "mark-unread";
    case "flag":
      return action.flagged ? "flag" : "unflag";
    case "junk":
      return action.junk ? "junk" : "not-junk";
    default:
      return action.type;
  }
}

/** `MailAction::removes_from_list`. */
export function removesFromList(action: MailAction): boolean {
  return !["mark-read", "flag", "tags"].includes(action.type);
}

interface Snapshot {
  message: MessageSummary;
  folder_id: FolderId;
  account_id: string;
  flags: MessageSummary["flags"];
  tags: string[];
}

interface Batch {
  kind: MailActionKind;
  snapshots: Snapshot[];
  snoozed: boolean;
  undone: boolean;
  until: number;
}

const batches = new WeakMap<MockData, Map<MailBatchId, Batch>>();

function batchesOf(d: MockData): Map<MailBatchId, Batch> {
  let map = batches.get(d);
  if (!map) {
    map = new Map();
    batches.set(d, map);
  }
  return map;
}

function issue(
  id: MailActionIssue,
  message: string,
  code: "invalid_request" | "not_found" | "conflict",
) {
  return {
    ...errorResponse(code, message),
    localized: { id: MAIL_ACTION_ISSUE_MESSAGE_IDS[id], args: {} },
  };
}

const roleOf = (d: MockData, folder: FolderId) => d.folders.find((f) => f.id === folder)?.role;

interface Resolved {
  messages: MessageSummary[];
  conversations: number;
}

/** `nail_api::mail_actions::explicit_target_count`: rows named one by one. */
export function explicitTargetCount(targets: readonly MessageTarget[]): number {
  return targets.reduce((n, t) => n + (t.kind === "list" ? explicitTargetCount(t.except) : 1), 0);
}

/** The message ids each `as_of` mark of this mock knew (`MailTargetResolution::as_of`). */
const marks = new WeakMap<MockData, Map<string, ReadonlySet<MessageId>>>();

/** A new `as_of` mark: the messages the mock holds now. */
function markNow(d: MockData): string {
  const known = marks.get(d) ?? new Map<string, ReadonlySet<MessageId>>();
  marks.set(d, known);
  const mark = `mock-mark-${String(known.size + 1)}`;
  known.set(mark, new Set(d.messages.map((m) => m.id)));
  return mark;
}

/** The rows of a whole list as the targets they stand for (`Service::mail_list_rows`). */
function listRows(d: MockData, list: MailList): RowTarget[] | Response {
  switch (list.source) {
    case "conversations": {
      const threads = listThreads(d, list.folder_id);
      if (!threads) return errorResponse("not_found", `folder ${list.folder_id} not found`);
      return threads.map((t) =>
        listRowTarget(list, { messageId: t.latest_message_id, threadId: t.id }),
      );
    }
    case "messages": {
      const messages = folderMessages(d, list.folder_id);
      if (!messages) return errorResponse("not_found", `folder ${list.folder_id} not found`);
      return messages.map((m) => listRowTarget(list, { messageId: m.id, threadId: m.thread_id }));
    }
    case "search": {
      const answer = search(d, { ...list.query, offset: 0, limit: Number.MAX_SAFE_INTEGER });
      if (answer.type === "InvalidQuery") {
        return errorResponse("invalid_request", `invalid query: ${answer.error.message}`);
      }
      if (answer.type !== "SearchResults") return answer;
      return answer.hits
        .filter((h) => h.from_server !== true)
        .map((h) =>
          listRowTarget(list, { messageId: h.message.id, threadId: h.message.thread_id }),
        );
    }
  }
}

interface Row {
  messages: MessageSummary[];
}

/**
 * The messages one row reaches (Trash's own messages left out for Trash).
 * A message no longer stored is `not_found`, or reaches nothing for a
 * whole list's rows and exceptions (`gone` = "skip", like the core).
 */
function resolveRow(
  d: MockData,
  target: RowTarget,
  action: MailAction,
  gone: "fail" | "skip" = "fail",
): Row | Response {
  if (target.kind === "message") {
    const m = d.messages.find((x) => x.id === target.message_id);
    if (!m && gone === "skip") return { messages: [] };
    if (!m) return errorResponse("not_found", `message ${target.message_id} not found`);
    const keep = action.type !== "trash" || roleOf(d, m.folder_id) !== "trash";
    return { messages: keep ? [m] : [] };
  }
  const answer = getConversation(d, target.thread_id, target.folder_id);
  if (answer.type !== "Conversation") return answer;
  const sources = target.folder_id === null ? null : (resolveFolder(d, target.folder_id) ?? []);
  const reach = reachOf(action);
  const picked: MessageSummary[] = answer.messages.filter((m) => {
    if (reach === "whole" || reach === "thread") return true;
    if (reach === "newest") return m.id === answer.thread.latest_message_id;
    return sources === null ? roleOf(d, m.folder_id) !== "sent" : sources.includes(m.folder_id);
  });
  if (reach === "thread") {
    // Gmail's Delete: the thread's messages in every other folder of the
    // listed accounts but Drafts, Trash and Junk.
    const accounts =
      sources === null
        ? null
        : new Set(sources.map((f) => d.folders.find((x) => x.id === f)?.account_id));
    for (const m of d.messages) {
      if (m.thread_id !== target.thread_id || picked.some((p) => p.id === m.id)) continue;
      if (["drafts", "trash", "junk"].includes(roleOf(d, m.folder_id) ?? "")) continue;
      if (accounts === null || accounts.has(m.account_id)) picked.push(m);
    }
  }
  // Trash never deletes for good: messages in Trash stay.
  const messages = picked
    .filter((m) => action.type !== "trash" || roleOf(d, m.folder_id) !== "trash")
    .flatMap((p) => d.messages.filter((x) => x.id === p.id));
  return { messages };
}

/**
 * Resolves targets like the core: per message, per conversation by reach,
 * or per row of a whole list minus its exceptions (R5, ADR 0235).
 */
function resolve(
  core: MockCore,
  targets: MessageTarget[],
  action: MailAction,
): Resolved | Response {
  const d = core.data;
  if (targets.length === 0) return issue("no-targets", "no targets", "invalid_request");
  if (explicitTargetCount(targets) > MAX_MAIL_ACTION_TARGETS) {
    return issue("too-many-targets", "too many targets", "invalid_request");
  }
  if (targets.some((t) => t.kind === "list" && t.except.some((e) => e.kind === "list"))) {
    return issue("nested-list", "a list among a list's exceptions", "invalid_request");
  }
  if (action.type === "tags" && action.add.length === 0 && action.remove.length === 0) {
    return issue("nothing-to-change", "nothing to change", "invalid_request");
  }
  const out: MessageSummary[] = [];
  const seen = new Set<MessageId>();
  // Distinct conversations: two rows of one (two search hits) count once.
  const threads = new Set<string>();
  const add = (messages: readonly MessageSummary[], row: RowTarget) => {
    if (row.kind === "conversation" && messages.length > 0) threads.add(row.thread_id);
    for (const m of messages) {
      if (!seen.has(m.id)) {
        seen.add(m.id);
        out.push(m);
      }
    }
  };
  for (const target of targets) {
    if (target.kind !== "list") {
      const row = resolveRow(d, target, action);
      if ("type" in row) return row;
      add(row.messages, target);
      continue;
    }
    const except = new Set<MessageId>();
    for (const e of target.except) {
      if (e.kind === "list") continue;
      const row = resolveRow(d, e, action, "skip");
      if ("type" in row) return row;
      for (const m of row.messages) except.add(m.id);
    }
    const known = target.as_of === null ? null : marks.get(d)?.get(target.as_of);
    const rows = listRows(d, target.list);
    if ("type" in rows) return rows;
    for (const r of rows) {
      const row = resolveRow(d, r, action, "skip");
      if ("type" in row) return row;
      const left = row.messages.filter((m) => !except.has(m.id) && (known?.has(m.id) ?? true));
      add(left, r);
    }
  }
  if (action.type === "delete-permanently") {
    const outside = out.find((m) => !["trash", "junk"].includes(roleOf(d, m.folder_id) ?? ""));
    if (outside) {
      return issue(
        "not-in-trash",
        `message ${outside.id} is not in Trash or Junk`,
        "invalid_request",
      );
    }
  }
  return { messages: out, conversations: threads.size };
}

function apply(core: MockCore, ids: MessageId[], action: MailAction): Response {
  switch (action.type) {
    case "archive":
      return moveMessages(core, ids, { role: "archive" });
    case "trash":
      return deleteMessages(core, ids, false);
    case "delete-permanently":
      return deleteMessages(core, ids, true);
    case "move":
      return moveMessages(core, ids, action.target);
    case "mark-read":
      return updateFlags(core, ids, flagChange({ seen: action.read }));
    case "flag":
      return updateFlags(core, ids, flagChange({ flagged: action.flagged }));
    case "tags":
      return updateFlags(
        core,
        ids,
        flagChange({ add_tags: action.add, remove_tags: action.remove }),
      );
    case "snooze":
      return answerTriage(core, {
        type: "SnoozeMessages",
        message_ids: ids,
        until: action.until,
        wake_folder_id: action.wake_folder_id,
      });
    case "junk":
      return answerTriage(core, { type: "MarkJunk", message_ids: ids, junk: action.junk });
  }
}

function flagChange(change: {
  seen?: boolean;
  flagged?: boolean;
  add_tags?: string[];
  remove_tags?: string[];
}) {
  return {
    seen: change.seen ?? null,
    flagged: change.flagged ?? null,
    answered: null,
    draft: null,
    add_tags: change.add_tags ?? [],
    remove_tags: change.remove_tags ?? [],
  };
}

/**
 * `MailAction::Move`: moving from Trash to an Inbox puts the user's own
 * messages (sent from the account's address) back into Sent, like the core.
 */
function restoreOwnToSent(
  core: MockCore,
  messages: MessageSummary[],
  target: MoveTarget,
): Response {
  const d = core.data;
  const own = (m: MessageSummary) => {
    if (roleOf(d, m.folder_id) !== "trash") return false;
    const email = d.accounts.find((a) => a.id === m.account_id)?.email.toLowerCase();
    if (email === undefined || m.from?.email.toLowerCase() !== email) return false;
    const to =
      "role" in target ? target.role : d.folders.find((f) => f.id === target.folder_id)?.role;
    return (
      to === "inbox" && d.folders.some((f) => f.account_id === m.account_id && f.role === "sent")
    );
  };
  const mine = messages.filter(own);
  const rest = messages.filter((m) => !mine.includes(m));
  const answers = [
    ...(rest.length > 0
      ? [
          moveMessages(
            core,
            rest.map((m) => m.id),
            target,
          ),
        ]
      : []),
    ...(mine.length > 0
      ? [
          moveMessages(
            core,
            mine.map((m) => m.id),
            { role: "sent" },
          ),
        ]
      : []),
  ];
  const failed = answers.find((a) => a.type !== "Queued");
  if (failed) return failed;
  return {
    type: "Queued",
    op_ids: answers.flatMap((a) => (a.type === "Queued" ? a.op_ids : [])),
  };
}

function applyMailAction(core: MockCore, targets: MessageTarget[], action: MailAction): Response {
  const d = core.data;
  const resolved = resolve(core, targets, action);
  if ("type" in resolved) return resolved;
  let messages = resolved.messages;
  if (action.type === "archive") {
    messages = messages.filter((m) => roleOf(d, m.folder_id) !== "archive");
  }
  if (messages.length === 0) return issue("no-messages", "no messages", "invalid_request");
  const permanent = action.type === "delete-permanently";
  const snapshots: Snapshot[] = messages.map((m) => ({
    message: m,
    folder_id: m.folder_id,
    account_id: m.account_id,
    flags: { ...m.flags },
    tags: [...m.tags],
  }));
  const ids = messages.map((m) => m.id);
  const answer =
    action.type === "move"
      ? restoreOwnToSent(core, messages, action.target)
      : apply(core, ids, action);
  if (answer.type !== "Queued") return answer;
  let batch_id: MailBatchId | null = null;
  let undo_until: string | null = null;
  if (!permanent) {
    batch_id = core.nextId("batch-");
    const until = Date.now() + MAIL_UNDO_SECS * 1000;
    undo_until = new Date(until).toISOString();
    batchesOf(d).set(batch_id, {
      kind: kindOf(action),
      snapshots,
      snoozed: action.type === "snooze",
      undone: false,
      until,
    });
  }
  const result: MailActionResult = {
    batch_id,
    op_ids: answer.op_ids,
    kind: kindOf(action),
    message_ids: ids,
    conversations: resolved.conversations,
    undo_until,
    left_out: 0,
  };
  return { type: "MailActionQueued", result };
}

function undoMailAction(core: MockCore, batchId: MailBatchId): Response {
  const d = core.data;
  const batch = batchesOf(d).get(batchId);
  if (!batch || batch.until < Date.now()) {
    return issue("undo-expired", `batch ${batchId} cannot be undone`, "not_found");
  }
  if (batch.undone) return issue("undone-already", `batch ${batchId} was undone`, "conflict");
  batch.undone = true;
  if (batch.snoozed) {
    const ids = batch.snapshots.map((s) => s.message.id);
    answerTriage(core, { type: "UnsnoozeMessages", message_ids: ids });
  }
  // Messages deleted since are skipped, like the core's stale inverses.
  const alive = batch.snapshots.filter((s) => d.messages.includes(s.message));
  if (alive.length === 0) return { type: "Queued", op_ids: [] };
  return queue(
    core,
    alive.map((s) => s.message),
    () => {
      const changed = new Set<FolderId>();
      for (const s of alive) {
        changed.add(s.message.folder_id);
        changed.add(s.folder_id);
        s.message.folder_id = s.folder_id;
        s.message.account_id = s.account_id;
        s.message.flags = { ...s.flags };
        s.message.tags = [...s.tags];
      }
      return [...changed];
    },
  );
}

/** Answers a mail-action request. */
/**
 * The messages a whole message list stands for when rules run on it
 * (`RunRules` with `list`, R8, ADR 0251): every row minus the exceptions,
 * only what the core held at `as_of`, a conversation row as its messages in
 * the listed folder (the "listed" reach, the one of `archive`).
 */
export function listTargetMessageIds(core: MockCore, list: ListTarget): MessageId[] | Response {
  const resolved = resolve(core, [{ kind: "list", ...list }], { type: "archive" });
  return "type" in resolved ? resolved : resolved.messages.map((m) => m.id);
}

export function answerMailAction(core: MockCore, request: MailActionRequest): Response {
  switch (request.type) {
    case "ApplyMailAction":
      return applyMailAction(core, request.targets, request.action);
    case "ResolveMailTargets": {
      const resolved = resolve(core, request.targets, request.action);
      if ("type" in resolved) return resolved;
      const list = request.targets.some((t) => t.kind === "list");
      return {
        type: "MailTargets",
        resolution: {
          // A list answers with the count and a mark (ADR 0235).
          message_ids: list ? [] : resolved.messages.map((m) => m.id),
          messages: resolved.messages.length,
          account_ids: [...new Set(resolved.messages.map((m) => m.account_id))],
          conversations: resolved.conversations,
          as_of: list ? markNow(core.data) : null,
        },
      };
    }
    case "UndoMailAction":
      return undoMailAction(core, request.batch_id);
  }
}
