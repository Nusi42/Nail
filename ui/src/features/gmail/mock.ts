/**
 * The mock core's answers to the M3b Gmail label requests (ADR 0065), used
 * by `MockTransport` (`../../api/mock.ts` routes exactly these requests
 * here).
 *
 * In-memory labels per Gmail account on `data.features.gmail`, close to the
 * core: system labels first (folders, then categories), then user labels
 * by name; counts from the stored messages (user labels are Nail tags, so a
 * new tag on a Gmail message shows up as a label); validation with the
 * contract's issues; creations get a `local-…` id that the "replay" turns
 * into a server id (`GmailLabelsChanged`); renaming renames the tags (and
 * nested labels), deleting removes the tag from the messages.
 *
 * `state.unsupported` makes every request answer `unsupported` (the core
 * before WP85). The demo account comes from `seedGmailDemo` (`mockSeed.ts`).
 */
import type {
  AccountId,
  GmailLabel,
  GmailLabelSettings,
  Request,
  Response,
} from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";
import { CATEGORIES, categoryLabel, isWithin, labelIssues } from "./model";

/** The requests this feature answers. */
export const GMAIL_REQUESTS = [
  "ListGmailLabels",
  "CreateGmailLabel",
  "UpdateGmailLabel",
  "DeleteGmailLabel",
] as const satisfies readonly Request["type"][];

export type GmailRequest = Extract<Request, { type: (typeof GMAIL_REQUESTS)[number] }>;

export function isGmailRequest(request: Request): request is GmailRequest {
  return (GMAIL_REQUESTS as readonly string[]).includes(request.type);
}

/** The mock's Gmail data. */
export interface GmailMockState {
  /** Labels by account id (counts are computed when listed). */
  labels: Record<string, GmailLabel[]>;
  /** Server ids of labels created locally (`local-…` → `Label_…`). */
  renamedIds: Record<string, string>;
  /** Answer every request `unsupported` (the core before WP85). */
  unsupported: boolean;
}

export function createGmailMockState(): GmailMockState {
  return { labels: {}, renamedIds: {}, unsupported: false };
}

const clone = <T>(value: T): T => structuredClone(value);

function state(core: MockCore): GmailMockState {
  return core.data.features.gmail;
}

/** The folder roles of system labels shown as folders. */
const FOLDER_LABELS: Record<string, string> = {
  INBOX: "inbox",
  SENT: "sent",
  DRAFT: "drafts",
  SPAM: "junk",
  TRASH: "trash",
};

function isGmailAccount(core: MockCore, accountId: AccountId): boolean {
  return core.data.accounts.some((a) => a.id === accountId && a.kind === "gmail");
}

function labelsOf(core: MockCore, accountId: AccountId): GmailLabel[] {
  const s = state(core);
  s.labels[accountId] ??= [];
  return s.labels[accountId];
}

function invalid(issue: string): Response {
  return {
    type: "Error",
    code: "invalid_request",
    message: `invalid label: ${issue}`,
    localized: { id: `gmail-label-issue-${issue}` },
  };
}

/** A user label for a tag a message got without a label (the core creates one). */
function adoptTags(core: MockCore, accountId: AccountId): void {
  const labels = labelsOf(core, accountId);
  const known = new Set(labels.map((l) => (l.tag ?? l.name).toLowerCase()));
  for (const m of core.data.messages) {
    if (m.account_id !== accountId) continue;
    for (const tag of m.tags) {
      if (tag.startsWith("$") || known.has(tag.toLowerCase())) continue;
      known.add(tag.toLowerCase());
      labels.push({
        account_id: accountId,
        id: core.nextId("local-"),
        name: tag,
        kind: "user",
        color: null,
        list_visibility: "show",
        message_visibility: "show",
        tag,
        folder_id: null,
        total: 0,
        unread: 0,
        sync: "pending",
      });
    }
  }
}

/** The labels with counts from the stored messages, in the contract's order. */
export function listLabels(core: MockCore, accountId: AccountId): GmailLabel[] {
  adoptTags(core, accountId);
  const messages = core.data.messages.filter((m) => m.account_id === accountId);
  const folders = core.data.folders.filter((f) => f.account_id === accountId);
  const counted = labelsOf(core, accountId).map((label) => {
    const mine = ((): typeof messages => {
      if (label.kind === "user") {
        const tag = (label.tag ?? label.name).toLowerCase();
        return messages.filter((m) => m.tags.some((t) => t.toLowerCase() === tag));
      }
      if (FOLDER_LABELS[label.id]) {
        const folder = folders.find((f) => f.role === FOLDER_LABELS[label.id]);
        return messages.filter((m) => m.folder_id === folder?.id);
      }
      const category = CATEGORIES.find((c) => categoryLabel(c) === label.id);
      return category
        ? messages.filter((m) => m.gmail?.category === category)
        : messages.filter((m) => m.gmail?.label_ids.includes(label.id));
    })();
    return {
      ...clone(label),
      total: mine.length,
      unread: mine.filter((m) => !m.flags.seen).length,
    };
  });
  const order = (l: GmailLabel) => {
    if (l.kind === "user") return 100;
    const folder = Object.keys(FOLDER_LABELS).indexOf(l.id);
    if (folder >= 0) return folder;
    const category = CATEGORIES.findIndex((c) => categoryLabel(c) === l.id);
    return category >= 0 ? 10 + category : 50;
  };
  return counted.sort((a, b) => order(a) - order(b) || a.name.localeCompare(b.name));
}

function find(core: MockCore, accountId: AccountId, id: string): GmailLabel | undefined {
  const real = state(core).renamedIds[id] ?? id;
  return labelsOf(core, accountId).find((l) => l.id === real);
}

/** Renames or removes a tag on every message of the account; the affected folders. */
function retag(core: MockCore, accountId: AccountId, from: string, to: string | null): string[] {
  const folders = new Set<string>();
  for (const m of core.data.messages) {
    if (m.account_id !== accountId) continue;
    const next = m.tags.flatMap((t) => {
      if (!isWithin(t.toLowerCase(), from.toLowerCase())) return [t];
      return to === null
        ? t.toLowerCase() === from.toLowerCase()
          ? []
          : [t]
        : [to + t.slice(from.length)];
    });
    if (next.join("\u0000") !== m.tags.join("\u0000")) {
      m.tags = [...new Set(next)].sort();
      folders.add(m.folder_id);
    }
  }
  return [...folders];
}

/** "Replays" a queued label operation: a created label gets its server id. */
function replay(core: MockCore, accountId: AccountId, labelId: string): string {
  const op = core.nextId("gmail-op-");
  core.later(() => {
    const label = labelsOf(core, accountId).find((l) => l.id === labelId);
    if (label) {
      if (label.id.startsWith("local-")) {
        const server = `Label_${label.id.slice("local-".length)}`;
        state(core).renamedIds[label.id] = server;
        label.id = server;
      }
      label.sync = "synced";
    }
    core.emit({ type: "GmailLabelsChanged", account_id: accountId });
  });
  return op;
}

function saved(core: MockCore, accountId: AccountId, label: GmailLabel, op: string): Response {
  const listedLabel = listLabels(core, accountId).find((l) => l.id === label.id) ?? label;
  return { type: "GmailLabelSaved", label: listedLabel, op_ids: [op] };
}

function check(
  core: MockCore,
  accountId: AccountId,
  settings: GmailLabelSettings,
  except: string | null,
): Response | null {
  const others = labelsOf(core, accountId).filter((l) => l.id !== except);
  const issue = labelIssues(settings, others)[0];
  return issue ? invalid(issue) : null;
}

export function answerGmail(core: MockCore, request: GmailRequest): Response {
  const s = state(core);
  if (s.unsupported) {
    return errorResponse("unsupported", `${request.type} is not wired yet (M3b WP85)`);
  }
  if (!core.data.accounts.some((a) => a.id === request.account_id)) {
    return errorResponse("not_found", `account ${request.account_id} not found`);
  }
  if (!isGmailAccount(core, request.account_id)) {
    return errorResponse("invalid_request", `${request.account_id} is not a Gmail account`);
  }
  const accountId = request.account_id;
  switch (request.type) {
    case "ListGmailLabels":
      return { type: "GmailLabels", account_id: accountId, labels: listLabels(core, accountId) };
    case "CreateGmailLabel": {
      const problem = check(core, accountId, request.settings, null);
      if (problem) return problem;
      const name = request.settings.name.trim();
      const label: GmailLabel = {
        account_id: accountId,
        id: core.nextId("local-"),
        name,
        kind: "user",
        color: request.settings.color ? clone(request.settings.color) : null,
        list_visibility: request.settings.list_visibility,
        message_visibility: request.settings.message_visibility,
        tag: name,
        folder_id: null,
        total: 0,
        unread: 0,
        sync: "pending",
      };
      labelsOf(core, accountId).push(label);
      const op = replay(core, accountId, label.id);
      core.emit({ type: "GmailLabelsChanged", account_id: accountId });
      return saved(core, accountId, label, op);
    }
    case "UpdateGmailLabel": {
      const label = find(core, accountId, request.label_id);
      if (!label) return errorResponse("not_found", `label ${request.label_id} not found`);
      if (label.kind === "system") {
        return errorResponse("invalid_request", "system labels cannot be changed");
      }
      const problem = check(core, accountId, request.settings, label.id);
      if (problem) return problem;
      const name = request.settings.name.trim();
      const old = label.name;
      let folders: string[] = [];
      if (name !== old) {
        // Nested labels move along (Gmail renames `Work/…` with `Work`).
        for (const other of labelsOf(core, accountId)) {
          if (other.kind === "user" && other !== label && isWithin(other.name, old)) {
            other.name = name + other.name.slice(old.length);
            other.tag = other.name;
            other.sync = "pending";
          }
        }
        folders = retag(core, accountId, old, name);
      }
      label.name = name;
      label.tag = name;
      label.color = request.settings.color ? clone(request.settings.color) : null;
      label.list_visibility = request.settings.list_visibility;
      label.message_visibility = request.settings.message_visibility;
      label.sync = "pending";
      const op = replay(core, accountId, label.id);
      core.emit({ type: "GmailLabelsChanged", account_id: accountId });
      if (folders.length > 0) core.emit({ type: "MailChanged", folder_ids: folders });
      return saved(core, accountId, label, op);
    }
    case "DeleteGmailLabel": {
      const label = find(core, accountId, request.label_id);
      if (!label) return errorResponse("not_found", `label ${request.label_id} not found`);
      if (label.kind === "system") {
        return errorResponse("invalid_request", "system labels cannot be deleted");
      }
      s.labels[accountId] = labelsOf(core, accountId).filter((l) => l !== label);
      const folders = retag(core, accountId, label.tag ?? label.name, null);
      core.emit({ type: "GmailLabelsChanged", account_id: accountId });
      if (folders.length > 0) core.emit({ type: "MailChanged", folder_ids: folders });
      return { type: "Done" };
    }
  }
}
