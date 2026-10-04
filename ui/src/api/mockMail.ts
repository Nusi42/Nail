/**
 * Mail behaviour of the mock core: virtual accounts (the built-in "All
 * accounts" plus user-defined ones), folder merging by role and by hand-made
 * mappings, Message-ID dedupe, queued writes with `OpStatus` events,
 * threads and search.
 */
import type {
  Account,
  AccountId,
  FlagChange,
  Folder,
  FolderId,
  FolderRole,
  MessageId,
  MessageLocation,
  MessageSummary,
  MoveTarget,
  Response,
  SearchQuery,
  ThreadSummary,
  VirtualAccountConfig,
} from "./generated";
import type { MockData } from "./mockData";
import { matches, parseQuery, type Searchable } from "./mockSearch";
import type { Event as NailEvent } from "./generated";

/** The built-in "All accounts" virtual account id (`AccountId::ALL`). */
export const ALL_ACCOUNTS = "all";

/** Roles merged across the members of a virtual account. */
export const MERGED_ROLES: readonly FolderRole[] = [
  "inbox",
  "sent",
  "drafts",
  "archive",
  "junk",
  "trash",
];

/** What the mock core needs from its transport. */
export interface MockCore {
  data: MockData;
  emit: (event: NailEvent) => void;
  /** Runs `fn` after the configured work delay (asynchronous core work). */
  later: (fn: () => void, ms?: number) => void;
  /** A fresh id with the given prefix. */
  nextId: (prefix: string) => string;
}

export function errorResponse(
  code: Extract<Response, { type: "Error" }>["code"],
  message: string,
): Response {
  return { type: "Error", code, message };
}

/** The built-in "All accounts" configuration: every enabled real account. */
export function allAccountsConfig(d: MockData): VirtualAccountConfig {
  return {
    id: ALL_ACCOUNTS,
    name: "",
    members: d.accountsConfig.accounts.filter((a) => a.enabled).map((a) => a.id),
    merge_roles: true,
    folder_mappings: [],
    default_identity: null,
    identity_rules: [],
  };
}

/** A virtual account's configuration, or `undefined` for real / unknown ids. */
export function virtualConfig(d: MockData, id: AccountId): VirtualAccountConfig | undefined {
  if (id === ALL_ACCOUNTS) return allAccountsConfig(d);
  return d.accountsConfig.virtual_accounts.find((v) => v.id === id);
}

/** `ListAccounts`: "All accounts", the real accounts, the user's virtual accounts. */
export function listAccounts(d: MockData): Account[] {
  return [
    { id: ALL_ACCOUNTS, name: "", email: "", kind: "virtual" },
    ...d.accounts,
    ...d.accountsConfig.virtual_accounts.map((v): Account => ({
      id: v.id,
      name: v.name,
      email: "",
      kind: "virtual",
    })),
  ];
}

interface VirtualFolder {
  folder: Folder;
  /** Real folders merged into it. */
  sources: FolderId[];
}

function counts(d: MockData, sources: readonly FolderId[]): { unread: number; total: number } {
  let unread = 0;
  let total = 0;
  for (const m of d.messages) {
    if (sources.includes(m.folder_id)) {
      total++;
      if (!m.flags.seen) unread++;
    }
  }
  return { unread, total };
}

/** The folders of a virtual account with the real folders behind each. */
export function virtualFolders(d: MockData, va: VirtualAccountConfig): VirtualFolder[] {
  const out: VirtualFolder[] = [];
  const used = new Set<FolderId>();
  const make = (
    id: string,
    name: string,
    role: FolderRole,
    parent: string | null,
    sources: FolderId[],
  ) => {
    sources.forEach((s) => used.add(s));
    out.push({
      folder: {
        id,
        account_id: va.id,
        parent_id: parent,
        name,
        path: parent ? `${parent.slice(va.id.length + 1)}/${name}` : name,
        role,
        ...counts(d, sources),
      },
      sources,
    });
  };
  const memberFolders = d.folders.filter((f) => va.members.includes(f.account_id));
  if (va.merge_roles) {
    for (const role of MERGED_ROLES) {
      const sources = memberFolders.filter((f) => f.role === role);
      if (sources.length === 0) continue;
      const name = sources[0]?.name ?? role;
      make(
        `${va.id}/${role}`,
        name,
        role,
        null,
        sources.map((f) => f.id),
      );
    }
  }
  va.folder_mappings.forEach((mapping, index) => {
    const sources = mapping.sources
      .map((s) => memberFolders.find((f) => f.account_id === s.account_id && f.path === s.path))
      .filter((f): f is Folder => f !== undefined)
      .map((f) => f.id);
    make(`${va.id}/map-${String(index)}`, mapping.name, "other", null, sources);
  });
  // Everything else under a per-account subtree, so nothing is hidden.
  for (const accountId of va.members) {
    const rest = memberFolders.filter((f) => f.account_id === accountId && !used.has(f.id));
    if (rest.length === 0) continue;
    const account = d.accounts.find((a) => a.id === accountId);
    const parent = `${va.id}/account-${accountId}`;
    make(parent, account?.name ?? accountId, "other", null, []);
    for (const f of rest) {
      make(`${parent}/${f.path}`, f.name, f.role, parent, [f.id]);
    }
  }
  return out;
}

/** `ListFolders` for any account. */
export function listFolders(d: MockData, accountId: AccountId): Folder[] | null {
  const va = virtualConfig(d, accountId);
  if (va) return virtualFolders(d, va).map((v) => v.folder);
  if (!d.accounts.some((a) => a.id === accountId)) return null;
  return d.folders
    .filter((f) => f.account_id === accountId)
    .map((f) => ({ ...f, ...counts(d, [f.id]) }));
}

/** Real folders behind a (real or virtual) folder id; `null` if unknown. */
export function resolveFolder(d: MockData, folderId: FolderId): FolderId[] | null {
  if (d.folders.some((f) => f.id === folderId)) return [folderId];
  const vaIds = [ALL_ACCOUNTS, ...d.accountsConfig.virtual_accounts.map((v) => v.id)];
  for (const vaId of vaIds) {
    if (!folderId.startsWith(`${vaId}/`)) continue;
    const va = virtualConfig(d, vaId);
    const found = va && virtualFolders(d, va).find((v) => v.folder.id === folderId);
    if (found) return found.sources;
  }
  return null;
}

function isVirtualFolder(d: MockData, folderId: FolderId): boolean {
  return !d.folders.some((f) => f.id === folderId);
}

function headerMessageId(d: MockData, id: MessageId): string {
  return d.bodies.get(id)?.headers.message_id ?? id;
}

/** Messages of a folder, newest first; virtual folders dedupe by Message-ID. */
export function folderMessages(d: MockData, folderId: FolderId): MessageSummary[] | null {
  const sources = resolveFolder(d, folderId);
  if (!sources) return null;
  const list = d.messages.filter((m) => sources.includes(m.folder_id));
  if (!isVirtualFolder(d, folderId)) return list;
  list.sort((a, b) => Date.parse(b.date) - Date.parse(a.date) || a.id.localeCompare(b.id));
  const byHeader = new Map<string, MessageSummary>();
  const out: MessageSummary[] = [];
  for (const m of list) {
    const key = headerMessageId(d, m.id);
    const first = byHeader.get(key);
    const location: MessageLocation = {
      message_id: m.id,
      account_id: m.account_id,
      folder_id: m.folder_id,
    };
    if (first) {
      first.duplicates.push(location);
    } else {
      const copy = { ...m, duplicates: [] as MessageLocation[] };
      byHeader.set(key, copy);
      out.push(copy);
    }
  }
  return out;
}

// ---------------------------------------------------------------------------
// Writes

export function findMessages(d: MockData, ids: readonly MessageId[]): MessageSummary[] | null {
  const found = ids.map((id) => d.messages.find((m) => m.id === id));
  return found.every((m) => m !== undefined) ? found : null;
}

/**
 * Queues one op per affected account, applies the change locally at once
 * (as the core does) and replays it asynchronously with `OpStatus` events.
 */
/** Applies a write at once and "replays" it per account (`Queued` + `OpStatus`). */
export function queue(
  core: MockCore,
  targets: MessageSummary[],
  apply: () => FolderId[],
): Response {
  const folders = apply();
  const accounts = [...new Set(targets.map((m) => m.account_id))];
  const ops = accounts.map((accountId) => ({ accountId, opId: core.nextId("op-") }));
  core.emit({ type: "MailChanged", folder_ids: folders });
  for (const { accountId, opId } of ops) replay(core, opId, accountId);
  return { type: "Queued", op_ids: ops.map((op) => op.opId) };
}

/** `RetryOp`: a failed write is queued and replayed again. */
export function retryOp(core: MockCore, opId: string): Response {
  const accountId = core.data.failedOps.get(opId);
  if (accountId === undefined) return errorResponse("conflict", `operation ${opId} has not failed`);
  core.data.failedOps.delete(opId);
  const base = { op_id: opId, account_id: accountId, error: null, message: null };
  queueMicrotask(() => {
    core.emit({ type: "OpStatus", status: { ...base, state: "pending" } });
  });
  replay(core, opId, accountId);
  return { type: "Queued", op_ids: [opId] };
}

/** Replays one queued write asynchronously with `OpStatus` events. */
function replay(core: MockCore, op_id: string, accountId: string): void {
  const base = { op_id, account_id: accountId, error: null, message: null };
  core.later(() => {
    core.emit({ type: "OpStatus", status: { ...base, state: "in-flight" } });
    core.later(() => {
      const error = core.data.behaviour.opError;
      if (error) core.data.failedOps.set(op_id, accountId);
      core.emit({
        type: "OpStatus",
        status: error
          ? { ...base, state: "failed", error, message: "mock replay failed" }
          : { ...base, state: "done" },
      });
    });
  });
}

export function updateFlags(core: MockCore, ids: MessageId[], change: FlagChange): Response {
  const targets = findMessages(core.data, ids);
  if (!targets) return errorResponse("not_found", "message not found");
  return queue(core, targets, () => {
    for (const m of targets) {
      m.flags = {
        seen: change.seen ?? m.flags.seen,
        flagged: change.flagged ?? m.flags.flagged,
        answered: change.answered ?? m.flags.answered,
        draft: change.draft ?? m.flags.draft,
      };
      const tags = new Set(m.tags);
      change.add_tags.forEach((t) => tags.add(t));
      change.remove_tags.forEach((t) => tags.delete(t));
      m.tags = [...tags].sort();
    }
    return [...new Set(targets.map((m) => m.folder_id))];
  });
}

/** The real folder a message moves to for `target`. */
function targetFolder(d: MockData, m: MessageSummary, target: MoveTarget): Folder | undefined {
  if ("role" in target) {
    return d.folders.find((f) => f.account_id === m.account_id && f.role === target.role);
  }
  const sources = resolveFolder(d, target.folder_id) ?? [];
  const candidates = d.folders.filter((f) => sources.includes(f.id));
  // A merged folder resolves to the source account's member folder.
  return candidates.find((f) => f.account_id === m.account_id) ?? candidates[0];
}

export function moveMessages(core: MockCore, ids: MessageId[], target: MoveTarget): Response {
  const d = core.data;
  const targets = findMessages(d, ids);
  if (!targets) return errorResponse("not_found", "message not found");
  const moves: { message: MessageSummary; to: Folder }[] = [];
  for (const message of targets) {
    const to = targetFolder(d, message, target);
    if (to === undefined) return errorResponse("not_found", "target folder not found");
    moves.push({ message, to });
  }
  return queue(core, targets, () => {
    const changed = new Set<FolderId>();
    for (const { message, to } of moves) {
      changed.add(message.folder_id);
      changed.add(to.id);
      message.folder_id = to.id;
      message.account_id = to.account_id;
    }
    return [...changed];
  });
}

export function deleteMessages(core: MockCore, ids: MessageId[], permanent: boolean): Response {
  const d = core.data;
  const targets = findMessages(d, ids);
  if (!targets) return errorResponse("not_found", "message not found");
  return queue(core, targets, () => {
    const changed = new Set<FolderId>();
    for (const m of targets) {
      changed.add(m.folder_id);
      const folder = d.folders.find((f) => f.id === m.folder_id);
      const trash = d.folders.find((f) => f.account_id === m.account_id && f.role === "trash");
      if (permanent || folder?.role === "trash" || !trash) {
        d.messages.splice(d.messages.indexOf(m), 1);
        d.bodies.delete(m.id);
      } else {
        m.folder_id = trash.id;
        changed.add(trash.id);
      }
    }
    return [...changed];
  });
}

// ---------------------------------------------------------------------------
// Threads

const threadKey = (m: MessageSummary) => m.thread_id ?? `single-${m.id}`;

function summarize(id: string, messages: MessageSummary[]): ThreadSummary {
  const sorted = [...messages].sort((a, b) => Date.parse(a.date) - Date.parse(b.date));
  const latest = sorted.at(-1);
  if (latest === undefined) throw new Error(`thread ${id} has no messages`);
  const participants = new Map<string, MessageSummary["from"]>();
  for (const m of sorted) if (m.from) participants.set(m.from.email, m.from);
  return {
    id,
    subject: (sorted[0]?.subject ?? "").replace(/^((re|fwd?|aw|wg):\s*)+/i, ""),
    participants: [...participants.values()].filter((a) => a !== null),
    message_count: messages.length,
    unread_count: messages.filter((m) => !m.flags.seen).length,
    flagged: messages.some((m) => m.flags.flagged),
    has_attachments: messages.some((m) => m.has_attachments),
    date: latest.date,
    preview: latest.preview,
    latest_message_id: latest.id,
    account_ids: [...new Set(messages.map((m) => m.account_id))],
  };
}

export function listThreads(d: MockData, folderId: FolderId): ThreadSummary[] | null {
  const messages = folderMessages(d, folderId);
  if (!messages) return null;
  const groups = new Map<string, MessageSummary[]>();
  for (const m of messages) {
    const key = threadKey(m);
    groups.set(key, [...(groups.get(key) ?? []), m]);
  }
  return [...groups]
    .map(([id, list]) => summarize(id, list))
    .sort((a, b) => Date.parse(b.date) - Date.parse(a.date));
}

export function getThread(d: MockData, threadId: string): Response {
  const messages = d.messages
    .filter((m) => threadKey(m) === threadId)
    .sort((a, b) => Date.parse(a.date) - Date.parse(b.date));
  if (messages.length === 0) return errorResponse("not_found", `thread ${threadId} not found`);
  return { type: "Thread", thread: summarize(threadId, messages), messages };
}

// ---------------------------------------------------------------------------
// Search

function searchable(d: MockData, m: MessageSummary): Searchable {
  const body = d.bodies.get(m.id);
  const address = (a: { name: string | null; email: string } | null) =>
    a ? `${a.name ?? ""} ${a.email}` : "";
  return {
    subject: m.subject,
    from: address(m.from),
    to: m.to.map(address).join(" "),
    text: body?.text ?? m.preview,
    tags: m.tags,
    account: m.account_id,
    accountName: d.accounts.find((a) => a.id === m.account_id)?.name ?? "",
    folder: d.folders.find((f) => f.id === m.folder_id)?.path ?? "",
    date: Date.parse(m.date),
    size: (body?.text.length ?? 0) + (body?.attachments.reduce((s, a) => s + a.size, 0) ?? 0),
    unread: !m.flags.seen,
    flagged: m.flags.flagged,
    answered: m.flags.answered,
    draft: m.flags.draft,
    hasAttachments: m.has_attachments,
    category: m.gmail?.category ?? null,
    focus: m.exchange?.inference ?? null,
  };
}

export function search(d: MockData, query: SearchQuery): Response {
  const { node, error } = parseQuery(query.text);
  if (error) return { type: "InvalidQuery", error };
  let pool: MessageSummary[];
  if (query.folder_id !== null) {
    const list = folderMessages(d, query.folder_id);
    if (!list) return errorResponse("not_found", `folder ${query.folder_id} not found`);
    pool = list;
  } else if (query.account_id !== null) {
    const va = virtualConfig(d, query.account_id);
    const members = va ? va.members : [query.account_id];
    pool = d.messages.filter((m) => members.includes(m.account_id));
  } else {
    pool = d.messages;
  }
  const hits = pool
    .filter((m) => node === null || matches(node, searchable(d, m)))
    .sort((a, b) => Date.parse(b.date) - Date.parse(a.date));
  return {
    type: "SearchResults",
    offset: query.offset,
    total: hits.length,
    hits: hits
      .slice(query.offset, query.offset + query.limit)
      .map((message, index) => ({ message, score: hits.length - index })),
  };
}
