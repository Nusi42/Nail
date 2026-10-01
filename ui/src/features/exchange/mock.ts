/**
 * The mock core's answers to the M3c Exchange requests (ADR 0072), used by
 * `MockTransport` (`../../api/mock.ts` routes exactly these requests here).
 *
 * An in-memory Exchange account on `data.features.exchange`, close to the
 * core: other mailboxes (an added one is `pending` until the "sync" finds
 * it in the directory — then its folder tree appears under `@<address>` —
 * or answers `not-found` / `access-denied`; removing an automapped one hides
 * it), the category master list with counts from the messages' tags
 * (case-insensitive; creations get a `local-…` id the "replay" turns into a
 * server id; renaming retags messages; deleting leaves the tags), Focused
 * Inbox moves (with sender overrides), automatic replies (queued, then
 * synced), rules (placements of the Nail rules computed from the rules mock,
 * the user's own server rules switched / deleted through the queue), room
 * lists and rooms with availability from daily busy windows, and the status.
 * Validation uses the contract's issue ids.
 *
 * `state.unsupported` makes every request answer `unsupported` (the core
 * before WP95). The demo account comes from `seedExchangeDemo`
 * (`mockSeed.ts`).
 */
import type {
  AccountId,
  AutomaticReplies,
  BusyPeriod,
  Condition,
  ExchangeCategory,
  ExchangeMailbox,
  ExchangeRuleStatus,
  ExchangeRulesStatus,
  ExchangeStatus,
  Request,
  Response,
  Room,
  RoomAvailability,
  RoomList,
  Rule,
} from "../../api/generated";
import { errorResponse, type MockCore } from "../../api/mockMail";
import { automaticRepliesIssues, categoryIssues, mailboxIssues } from "./model";

/** The requests this feature answers. */
export const EXCHANGE_REQUESTS = [
  "GetExchangeStatus",
  "ListExchangeMailboxes",
  "AddExchangeMailbox",
  "UpdateExchangeMailbox",
  "RemoveExchangeMailbox",
  "ListExchangeCategories",
  "CreateExchangeCategory",
  "UpdateExchangeCategory",
  "DeleteExchangeCategory",
  "ClassifyFocused",
  "GetAutomaticReplies",
  "SetAutomaticReplies",
  "GetExchangeRules",
  "SyncExchangeRules",
  "SetExchangeInboxRuleEnabled",
  "DeleteExchangeInboxRule",
  "ListRoomLists",
  "FindRooms",
] as const satisfies readonly Request["type"][];

export type ExchangeRequest = Extract<Request, { type: (typeof EXCHANGE_REQUESTS)[number] }>;

export function isExchangeRequest(request: Request): request is ExchangeRequest {
  return (EXCHANGE_REQUESTS as readonly string[]).includes(request.type);
}

/** A mailbox the mock "directory" knows (`access` false = the user has no permission). */
export interface DirectoryEntry {
  name: string;
  access: boolean;
  /** Folder names of its mail tree (under `@<address>`). */
  folders: string[];
}

/** A room's busy time every day (UTC hours, `[start, end)`). */
export interface DailyBusy {
  start: number;
  end: number;
}

/** The mock's Exchange data by account id. */
export interface ExchangeMockState {
  mailboxes: Record<string, ExchangeMailbox[]>;
  /** Automapped mailboxes the user removed (hidden, not deleted). */
  hiddenMailboxes: Record<string, string[]>;
  categories: Record<string, ExchangeCategory[]>;
  automaticReplies: Record<string, AutomaticReplies>;
  rules: Record<string, ExchangeRulesStatus>;
  roomLists: Record<string, RoomList[]>;
  rooms: Record<string, Room[]>;
  status: Record<string, ExchangeStatus>;
  /** Mailboxes that can be opened, by lowercase address. */
  directory: Record<string, DirectoryEntry>;
  /** Busy windows of rooms by lowercase address. */
  roomBusy: Record<string, DailyBusy[]>;
  /** Focused Inbox overrides by lowercase sender. */
  focusOverrides: Record<string, "focused" | "other">;
  /** Accounts whose tenant did not grant `Place.Read.All`. */
  roomsForbidden: string[];
  /** Answer every request `unsupported` (the core before WP95). */
  unsupported: boolean;
}

export function createExchangeMockState(): ExchangeMockState {
  return {
    mailboxes: {},
    hiddenMailboxes: {},
    categories: {},
    automaticReplies: {},
    rules: {},
    roomLists: {},
    rooms: {},
    status: {},
    directory: {},
    roomBusy: {},
    focusOverrides: {},
    roomsForbidden: [],
    unsupported: false,
  };
}

const clone = <T>(value: T): T => structuredClone(value);

function state(core: MockCore): ExchangeMockState {
  return core.data.features.exchange;
}

function invalid(message: string, id: string): Response {
  return { type: "Error", code: "invalid_request", message, localized: { id } };
}

function changed(core: MockCore, accountId: AccountId, areas: ExchangeChangedArea[]): void {
  core.emit({ type: "ExchangeChanged", account_id: accountId, areas });
}

type ExchangeChangedArea = Extract<
  Parameters<MockCore["emit"]>[0],
  { type: "ExchangeChanged" }
>["areas"][number];

function accountEmail(core: MockCore, accountId: AccountId): string {
  return core.data.accounts.find((a) => a.id === accountId)?.email ?? "";
}

function statusOf(core: MockCore, accountId: AccountId): ExchangeStatus {
  const s = state(core);
  s.status[accountId] ??= {
    account_id: accountId,
    engine: "graph",
    server_version: null,
    push: "polling",
    last_sync_at: null,
    throttled_until: null,
    folders_synced: 0,
    folders_total: 0,
    pending_ops: 0,
    warnings: [],
  };
  return s.status[accountId];
}

/** Queues an Exchange operation: counted as pending until `apply` runs later. */
function enqueue(core: MockCore, accountId: AccountId, apply: () => void): string {
  const op = core.nextId("exchange-op-");
  statusOf(core, accountId).pending_ops++;
  core.later(() => {
    const status = statusOf(core, accountId);
    status.pending_ops = Math.max(0, status.pending_ops - 1);
    apply();
    core.emit({
      type: "OpStatus",
      status: { op_id: op, account_id: accountId, state: "done", error: null, message: null },
    });
  });
  return op;
}

// ---------------------------------------------------------------------------
// Mailboxes

function mailboxesOf(core: MockCore, accountId: AccountId): ExchangeMailbox[] {
  const s = state(core);
  s.mailboxes[accountId] ??= [];
  return s.mailboxes[accountId];
}

function findMailbox(core: MockCore, accountId: AccountId, address: string) {
  const lower = address.trim().toLowerCase();
  return mailboxesOf(core, accountId).find((m) => m.settings.address.toLowerCase() === lower);
}

function visibleMailboxes(core: MockCore, accountId: AccountId): ExchangeMailbox[] {
  const hidden = state(core).hiddenMailboxes[accountId] ?? [];
  return mailboxesOf(core, accountId)
    .filter((m) => !hidden.includes(m.settings.address.toLowerCase()))
    .map(clone)
    .sort((a, b) =>
      (a.settings.name ?? a.settings.address).localeCompare(b.settings.name ?? b.settings.address),
    );
}

/** The mailbox's folder tree under the account (root `@<address>`). */
function addMailboxFolders(core: MockCore, accountId: AccountId, mailbox: ExchangeMailbox): void {
  const address = mailbox.settings.address.toLowerCase();
  const entry = state(core).directory[address];
  const rootId = `${accountId}/@${address}`;
  if (!core.data.folders.some((f) => f.id === rootId)) {
    core.data.folders.push({
      id: rootId,
      account_id: accountId,
      parent_id: null,
      name: mailbox.settings.name ?? entry?.name ?? address,
      path: `@${address}`,
      role: "other",
      unread: 0,
      total: 0,
    });
    for (const name of entry?.folders ?? ["Inbox"]) {
      core.data.folders.push({
        id: `${rootId}/${name}`,
        account_id: accountId,
        parent_id: rootId,
        name,
        path: `@${address}/${name}`,
        role: "other",
        unread: 0,
        total: 0,
      });
    }
  }
  mailbox.root_folder_id = rootId;
}

/** Folders appeared or went: the account's folder list (and its lists) refetch. */
function folderChange(core: MockCore, accountId: AccountId): void {
  const ids = core.data.folders.filter((f) => f.account_id === accountId).map((f) => f.id);
  core.emit({ type: "MailChanged", folder_ids: ids });
}

function removeMailboxFolders(core: MockCore, accountId: AccountId, address: string): void {
  const prefix = `@${address.toLowerCase()}`;
  const gone = new Set(
    core.data.folders
      .filter(
        (f) => f.account_id === accountId && (f.path === prefix || f.path.startsWith(`${prefix}/`)),
      )
      .map((f) => f.id),
  );
  core.data.folders = core.data.folders.filter((f) => !gone.has(f.id));
  core.data.messages = core.data.messages.filter((m) => !gone.has(m.folder_id));
}

/** The "sync" of a mailbox: found and allowed → synced (with folders), else an error state. */
function syncMailbox(core: MockCore, accountId: AccountId, address: string): void {
  core.later(() => {
    const mailbox = findMailbox(core, accountId, address);
    if (!mailbox) return;
    const entry = state(core).directory[address.toLowerCase()];
    if (!entry) {
      mailbox.state = "not-found";
      mailbox.error = "not_found";
      mailbox.localized = { id: "exchange-mailbox-state-not-found" };
    } else if (!entry.access) {
      mailbox.state = "access-denied";
      mailbox.error = "forbidden";
      mailbox.localized = { id: "exchange-mailbox-state-access-denied" };
    } else {
      mailbox.state = "synced";
      mailbox.error = null;
      delete mailbox.localized;
      if (mailbox.settings.mail) addMailboxFolders(core, accountId, mailbox);
      else {
        removeMailboxFolders(core, accountId, address);
        mailbox.root_folder_id = null;
      }
      if (mailbox.settings.name === null) mailbox.settings.name = entry.name;
    }
    changed(core, accountId, ["mailboxes"]);
    folderChange(core, accountId);
  });
}

// ---------------------------------------------------------------------------
// Categories

function categoriesOf(core: MockCore, accountId: AccountId): ExchangeCategory[] {
  const s = state(core);
  s.categories[accountId] ??= [];
  return s.categories[accountId];
}

/** The master list with counts from the stored messages' tags, by name. */
export function listCategories(core: MockCore, accountId: AccountId): ExchangeCategory[] {
  const messages = core.data.messages.filter((m) => m.account_id === accountId);
  return categoriesOf(core, accountId)
    .map((c) => {
      const name = c.name.toLowerCase();
      const tagged = messages.filter((m) => m.tags.some((t) => t.toLowerCase() === name));
      return {
        ...clone(c),
        total: tagged.length,
        unread: tagged.filter((m) => !m.flags.seen).length,
      };
    })
    .sort((a, b) => a.name.localeCompare(b.name));
}

/** Renames a tag on every message of the account (case-insensitive); the affected folders. */
function retag(core: MockCore, accountId: AccountId, from: string, to: string): string[] {
  const folders = new Set<string>();
  const lower = from.toLowerCase();
  for (const m of core.data.messages) {
    if (m.account_id !== accountId || !m.tags.some((t) => t.toLowerCase() === lower)) continue;
    m.tags = [...new Set(m.tags.map((t) => (t.toLowerCase() === lower ? to : t)))].sort();
    folders.add(m.folder_id);
  }
  return [...folders];
}

// ---------------------------------------------------------------------------
// Rules

/** Where a Nail rule would run: queries and tags / ages stay in Nail (like the Sieve mapping). */
function placementOf(rule: Rule): Pick<ExchangeRuleStatus, "placement" | "reasons"> {
  const reasons: ExchangeRuleStatus["reasons"] = [];
  const walk = (c: Condition, position: number): void => {
    switch (c.type) {
      case "all":
      case "any":
        c.conditions.forEach((inner, i) => walk(inner, position * 10 + i + 1));
        return;
      case "not":
        walk(c.condition, position);
        return;
      case "query":
        reasons.push({ id: "sieve-reason-query", args: { position } });
        return;
      case "tag":
        reasons.push({ id: "sieve-reason-tag", args: { position } });
        return;
      case "age":
        reasons.push({ id: "sieve-reason-age", args: { position } });
        return;
      default:
        return;
    }
  };
  walk(rule.conditions, 0);
  const arrival = rule.triggers.some((t) => t.type === "arrival");
  if (!arrival) return { placement: "client", reasons };
  return reasons.length > 0 ? { placement: "client", reasons } : { placement: "server", reasons };
}

function rulesOf(core: MockCore, accountId: AccountId): ExchangeRulesStatus {
  const s = state(core);
  s.rules[accountId] ??= {
    account_id: accountId,
    state: "in-sync",
    rules: [],
    server_rules: [],
    synced_at: null,
    error: null,
  };
  const status = s.rules[accountId];
  // Placements follow the current Nail rules (in rules.kdl order).
  const nail = core.data.features.rules.rules.filter(
    (r) =>
      r.scope.length === 0 ||
      r.scope.some((sc) => sc.type === "account" && sc.account_id === accountId),
  );
  status.rules = nail.map((rule) => {
    const placed = status.server_rules.find((r) => r.nail_rule_id === rule.id);
    const { placement, reasons } = placementOf(rule);
    return {
      rule_id: rule.id,
      placement,
      server_rule_id: placement === "server" ? (placed?.id ?? null) : null,
      reasons,
    };
  });
  return status;
}

// ---------------------------------------------------------------------------
// Rooms

const DAY = 86_400_000;
const MAX_RANGE_DAYS = 92;

function busyPeriods(core: MockCore, address: string, from: number, to: number): BusyPeriod[] {
  const windows = state(core).roomBusy[address.toLowerCase()] ?? [];
  const out: BusyPeriod[] = [];
  for (let day = Math.floor(from / DAY) * DAY; day < to; day += DAY) {
    for (const w of windows) {
      const start = day + w.start * 3_600_000;
      const end = day + w.end * 3_600_000;
      if (start < to && end > from) {
        out.push({
          start: new Date(Math.max(start, from)).toISOString().replace(/\.\d{3}Z$/, "Z"),
          end: new Date(Math.min(end, to)).toISOString().replace(/\.\d{3}Z$/, "Z"),
          kind: "busy",
        });
      }
    }
  }
  return out.sort((a, b) => a.start.localeCompare(b.start));
}

// ---------------------------------------------------------------------------

/** Answers like the core (ADR 0072). */
export function answerExchange(core: MockCore, request: ExchangeRequest): Response {
  const s = state(core);
  if (s.unsupported) {
    return errorResponse("unsupported", `${request.type} is not wired yet (M3c WP95)`);
  }
  if (request.type === "ClassifyFocused") return classify(core, request);
  const accountId = request.account_id;
  const account = core.data.accounts.find((a) => a.id === accountId);
  if (!account) return errorResponse("not_found", `account ${accountId} not found`);
  if (account.kind !== "exchange") {
    return errorResponse("invalid_request", `${accountId} is not an Exchange account`);
  }
  switch (request.type) {
    case "GetExchangeStatus":
      return { type: "ExchangeStatus", status: clone(statusOf(core, accountId)) };

    case "ListExchangeMailboxes":
      return {
        type: "ExchangeMailboxes",
        account_id: accountId,
        mailboxes: visibleMailboxes(core, accountId),
      };
    case "AddExchangeMailbox": {
      const address = request.settings.address.trim().toLowerCase();
      const hidden = s.hiddenMailboxes[accountId] ?? [];
      const existing = findMailbox(core, accountId, address);
      if (existing && hidden.includes(address)) {
        // Showing a hidden automapped mailbox again.
        s.hiddenMailboxes[accountId] = hidden.filter((a) => a !== address);
        existing.settings = { ...clone(request.settings), address };
        existing.state = "pending";
        syncMailbox(core, accountId, address);
        changed(core, accountId, ["mailboxes"]);
        return { type: "ExchangeMailboxSaved", mailbox: clone(existing) };
      }
      const issue = mailboxIssues(
        request.settings,
        accountEmail(core, accountId),
        mailboxesOf(core, accountId),
      )[0];
      if (issue) return invalid(`mailbox: ${issue}`, `exchange-mailbox-issue-${issue}`);
      const mailbox: ExchangeMailbox = {
        account_id: accountId,
        settings: {
          ...clone(request.settings),
          address,
          name: request.settings.name?.trim() ?? null,
        },
        origin: "user",
        state: "pending",
        root_folder_id: null,
        calendar_ids: [],
        address_book_ids: [],
        error: null,
      };
      mailboxesOf(core, accountId).push(mailbox);
      syncMailbox(core, accountId, address);
      changed(core, accountId, ["mailboxes"]);
      return { type: "ExchangeMailboxSaved", mailbox: clone(mailbox) };
    }
    case "UpdateExchangeMailbox": {
      const mailbox = findMailbox(core, accountId, request.settings.address);
      if (!mailbox) return errorResponse("not_found", `mailbox ${request.settings.address}`);
      const others = mailboxesOf(core, accountId).filter((m) => m !== mailbox);
      const issue = mailboxIssues(request.settings, accountEmail(core, accountId), others)[0];
      if (issue) return invalid(`mailbox: ${issue}`, `exchange-mailbox-issue-${issue}`);
      mailbox.settings = {
        ...clone(request.settings),
        address: mailbox.settings.address,
        name: request.settings.name?.trim() ?? null,
      };
      if (mailbox.settings.mail && mailbox.state === "synced") {
        addMailboxFolders(core, accountId, mailbox);
      } else if (!mailbox.settings.mail) {
        removeMailboxFolders(core, accountId, mailbox.settings.address);
        mailbox.root_folder_id = null;
      }
      const root = core.data.folders.find((f) => f.id === mailbox.root_folder_id);
      if (root && mailbox.settings.name) root.name = mailbox.settings.name;
      changed(core, accountId, ["mailboxes"]);
      folderChange(core, accountId);
      return { type: "ExchangeMailboxSaved", mailbox: clone(mailbox) };
    }
    case "RemoveExchangeMailbox": {
      const mailbox = findMailbox(core, accountId, request.address);
      if (!mailbox) return errorResponse("not_found", `mailbox ${request.address}`);
      const address = mailbox.settings.address.toLowerCase();
      removeMailboxFolders(core, accountId, address);
      if (mailbox.origin === "automapped") {
        s.hiddenMailboxes[accountId] = [...(s.hiddenMailboxes[accountId] ?? []), address];
        mailbox.root_folder_id = null;
      } else {
        s.mailboxes[accountId] = mailboxesOf(core, accountId).filter((m) => m !== mailbox);
      }
      changed(core, accountId, ["mailboxes"]);
      folderChange(core, accountId);
      return { type: "Done" };
    }

    case "ListExchangeCategories":
      return {
        type: "ExchangeCategories",
        account_id: accountId,
        categories: listCategories(core, accountId),
      };
    case "CreateExchangeCategory": {
      const issue = categoryIssues(request.settings, categoriesOf(core, accountId))[0];
      if (issue) return invalid(`category: ${issue}`, `exchange-category-issue-${issue}`);
      const category: ExchangeCategory = {
        account_id: accountId,
        id: core.nextId("local-"),
        name: request.settings.name.trim(),
        color: request.settings.color,
        total: 0,
        unread: 0,
        sync: "pending",
      };
      categoriesOf(core, accountId).push(category);
      const op = enqueue(core, accountId, () => {
        if (category.id.startsWith("local-")) {
          category.id = `{${category.id.slice("local-".length).padStart(8, "0")}-cat}`;
        }
        category.sync = "synced";
        changed(core, accountId, ["categories"]);
      });
      changed(core, accountId, ["categories"]);
      const listed = listCategories(core, accountId).find((c) => c.id === category.id);
      return { type: "ExchangeCategorySaved", category: listed ?? clone(category), op_ids: [op] };
    }
    case "UpdateExchangeCategory": {
      const category = categoriesOf(core, accountId).find((c) => c.id === request.category_id);
      if (!category) return errorResponse("not_found", `category ${request.category_id}`);
      const others = categoriesOf(core, accountId).filter((c) => c !== category);
      const issue = categoryIssues(request.settings, others)[0];
      if (issue) return invalid(`category: ${issue}`, `exchange-category-issue-${issue}`);
      const name = request.settings.name.trim();
      const folders = name === category.name ? [] : retag(core, accountId, category.name, name);
      category.name = name;
      category.color = request.settings.color;
      category.sync = "pending";
      const op = enqueue(core, accountId, () => {
        category.sync = "synced";
        changed(core, accountId, ["categories"]);
      });
      changed(core, accountId, ["categories"]);
      if (folders.length > 0) core.emit({ type: "MailChanged", folder_ids: folders });
      const listed = listCategories(core, accountId).find((c) => c.id === category.id);
      return { type: "ExchangeCategorySaved", category: listed ?? clone(category), op_ids: [op] };
    }
    case "DeleteExchangeCategory": {
      const category = categoriesOf(core, accountId).find((c) => c.id === request.category_id);
      if (!category) return errorResponse("not_found", `category ${request.category_id}`);
      s.categories[accountId] = categoriesOf(core, accountId).filter((c) => c !== category);
      const op = enqueue(core, accountId, () => changed(core, accountId, ["categories"]));
      changed(core, accountId, ["categories"]);
      return { type: "Queued", op_ids: [op] };
    }

    case "GetAutomaticReplies":
      return { type: "AutomaticReplies", replies: clone(repliesOf(core, accountId)), op_ids: [] };
    case "SetAutomaticReplies": {
      const issue = automaticRepliesIssues(request.settings)[0];
      if (issue) return invalid(`automatic replies: ${issue}`, `exchange-oof-issue-${issue}`);
      const replies = repliesOf(core, accountId);
      replies.settings = clone(request.settings);
      replies.sync = "pending";
      const op = enqueue(core, accountId, () => {
        replies.sync = "synced";
        replies.fetched_at = new Date().toISOString().replace(/\.\d{3}Z$/, "Z");
        changed(core, accountId, ["automatic-replies"]);
      });
      changed(core, accountId, ["automatic-replies"]);
      return { type: "AutomaticReplies", replies: clone(replies), op_ids: [op] };
    }

    case "GetExchangeRules":
      return { type: "ExchangeRules", status: clone(rulesOf(core, accountId)) };
    case "SyncExchangeRules": {
      const status = rulesOf(core, accountId);
      status.state = "syncing";
      core.later(() => {
        const now = rulesOf(core, accountId);
        // Place the expressible Nail rules that are not on the server yet.
        for (const placed of now.rules) {
          if (placed.placement !== "server" || placed.server_rule_id !== null) continue;
          const rule = core.data.features.rules.rules.find((r) => r.id === placed.rule_id);
          now.server_rules.push(
            serverRuleFor(core, now, rule?.name ?? placed.rule_id, placed.rule_id),
          );
        }
        now.state = "in-sync";
        now.error = null;
        delete now.localized;
        now.synced_at = new Date().toISOString().replace(/\.\d{3}Z$/, "Z");
        changed(core, accountId, ["rules"]);
      });
      changed(core, accountId, ["rules"]);
      return { type: "ExchangeRules", status: clone(rulesOf(core, accountId)) };
    }
    case "SetExchangeInboxRuleEnabled":
    case "DeleteExchangeInboxRule": {
      const status = rulesOf(core, accountId);
      const rule = status.server_rules.find((r) => r.id === request.server_rule_id);
      if (!rule) return errorResponse("not_found", `inbox rule ${request.server_rule_id}`);
      if (rule.in_error && request.type === "SetExchangeInboxRuleEnabled") {
        return errorResponse("invalid_request", "the server marked this rule broken");
      }
      if (request.type === "SetExchangeInboxRuleEnabled") rule.enabled = request.enabled;
      else status.server_rules = status.server_rules.filter((r) => r !== rule);
      const op = enqueue(core, accountId, () => changed(core, accountId, ["rules"]));
      changed(core, accountId, ["rules"]);
      return { type: "Queued", op_ids: [op] };
    }

    case "ListRoomLists": {
      if (s.roomsForbidden.includes(accountId)) return roomsForbidden();
      return {
        type: "RoomLists",
        account_id: accountId,
        lists: clone(s.roomLists[accountId] ?? []).sort((a, b) => a.name.localeCompare(b.name)),
      };
    }
    case "FindRooms": {
      if (s.roomsForbidden.includes(accountId)) return roomsForbidden();
      const from = Date.parse(request.from);
      const to = Date.parse(request.to);
      if (Number.isNaN(from) || Number.isNaN(to) || to <= from) {
        return invalid("free/busy: invalid-range", "freebusy-issue-invalid-range");
      }
      if (to - from > MAX_RANGE_DAYS * DAY) {
        return invalid("free/busy: range-too-long", "freebusy-issue-range-too-long");
      }
      const rooms = (s.rooms[accountId] ?? []).filter(
        (r) =>
          (request.room_list === null || r.room_list === request.room_list) &&
          (request.min_capacity === null || (r.capacity ?? 0) >= request.min_capacity),
      );
      const answered: RoomAvailability[] = rooms.map((room) => {
        if (!(room.address.toLowerCase() in s.roomBusy)) {
          return { room: clone(room), free: null, periods: [], error: "network" };
        }
        const periods = busyPeriods(core, room.address, from, to);
        return { room: clone(room), free: periods.length === 0, periods, error: null };
      });
      const rank = (r: RoomAvailability) => (r.free === true ? 0 : r.free === false ? 1 : 2);
      answered.sort((a, b) => rank(a) - rank(b) || a.room.name.localeCompare(b.room.name));
      return {
        type: "Rooms",
        account_id: accountId,
        from: request.from,
        to: request.to,
        rooms: answered,
      };
    }
  }
}

function roomsForbidden(): Response {
  return {
    type: "Error",
    code: "forbidden",
    message: "rooms need Place.Read.All",
    localized: { id: "exchange-warning-rooms-need-consent" },
  };
}

function serverRuleFor(
  core: MockCore,
  status: ExchangeRulesStatus,
  name: string,
  nailRuleId: string | null,
): ExchangeRulesStatus["server_rules"][number] {
  const sequence = Math.max(0, ...status.server_rules.map((r) => r.sequence)) + 1;
  return {
    id: core.nextId("inbox-rule-"),
    name,
    sequence,
    enabled: true,
    conditions: emptyConditions(),
    exceptions: emptyConditions(),
    actions: emptyActions(),
    has_unsupported: false,
    in_error: false,
    nail_rule_id: nailRuleId,
  };
}

export function emptyConditions(): ExchangeRulesStatus["server_rules"][number]["conditions"] {
  return {
    from_addresses: [],
    sent_to_addresses: [],
    subject_contains: [],
    body_contains: [],
    body_or_subject_contains: [],
    header_contains: [],
    sender_contains: [],
    recipient_contains: [],
    categories: [],
    has_attachments: null,
    importance: null,
    sent_to_me: null,
    sent_only_to_me: null,
    sent_cc_me: null,
    sent_to_or_cc_me: null,
    not_sent_to_me: null,
    is_meeting_request: null,
    min_size_kb: null,
    max_size_kb: null,
  };
}

export function emptyActions(): ExchangeRulesStatus["server_rules"][number]["actions"] {
  return {
    move_to_folder: null,
    copy_to_folder: null,
    delete: false,
    permanent_delete: false,
    mark_as_read: false,
    mark_importance: null,
    assign_categories: [],
    forward_to: [],
    forward_as_attachment_to: [],
    redirect_to: [],
    stop_processing_rules: false,
  };
}

function repliesOf(core: MockCore, accountId: AccountId): AutomaticReplies {
  const s = state(core);
  s.automaticReplies[accountId] ??= {
    account_id: accountId,
    settings: {
      status: "disabled",
      start: null,
      end: null,
      internal_reply: "",
      external_reply: "",
      external_audience: "all",
    },
    sync: "synced",
    fetched_at: null,
  };
  return s.automaticReplies[accountId];
}

function classify(
  core: MockCore,
  request: Extract<ExchangeRequest, { type: "ClassifyFocused" }>,
): Response {
  const messages = request.message_ids.map((id) => core.data.messages.find((m) => m.id === id));
  if (messages.some((m) => m === undefined)) {
    return errorResponse("not_found", "message not found");
  }
  const found = messages as NonNullable<(typeof messages)[number]>[];
  const accounts = new Set(found.map((m) => m.account_id));
  const exchange = core.data.accounts.filter((a) => a.kind === "exchange").map((a) => a.id);
  if (found.some((m) => !m.exchange || !exchange.includes(m.account_id)) || accounts.size > 1) {
    return errorResponse("invalid_request", "Focused Inbox only sorts Exchange inbox mail");
  }
  const accountId = found[0]?.account_id ?? "";
  for (const m of found) {
    if (m.exchange) m.exchange = { ...m.exchange, inference: request.classification };
    if (request.always && m.from) {
      state(core).focusOverrides[m.from.email.toLowerCase()] = request.classification;
    }
  }
  const op = enqueue(core, accountId, () => undefined);
  core.emit({ type: "MailChanged", folder_ids: [...new Set(found.map((m) => m.folder_id))] });
  return { type: "Queued", op_ids: [op] };
}
