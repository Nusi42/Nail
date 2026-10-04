/**
 * Pure helpers of the Exchange UI: validation mirrors of the contract's
 * `issues()` (immediate feedback with the same `exchange-*-issue-*` texts;
 * the core stays authoritative), category colours, Focused Inbox queries,
 * mailbox folder paths (`@<address>/…`), Autodiscover review (domain
 * redirects, EWS against Exchange Online), Exchange sign-in methods, rooms
 * as attendees and room availability, task-list grouping (To Do lists).
 */
import type {
  Account,
  AccountConfig,
  Attendee,
  AuthConfig,
  AutomaticRepliesIssue,
  AutomaticRepliesSettings,
  BusyPeriod,
  Calendar,
  CategoryColor,
  DiscoveredAccount,
  ExchangeCategory,
  ExchangeCategoryIssue,
  ExchangeCategorySettings,
  ExchangeEngine,
  ExchangeMailbox,
  ExchangeMailboxIssue,
  ExchangeMailboxSettings,
  ExchangeOptions,
  ExchangeWarning,
  Folder,
  InferenceClassification,
  RoomAvailability,
  SecretInput,
  SspiPackage,
} from "../../api/generated";
import { charCount } from "../../text/chars";

// ---------------------------------------------------------------------------
// Accounts

/** Whether an account is an Exchange account. */
export function isExchange(account: Pick<Account, "kind"> | undefined | null): boolean {
  return account?.kind === "exchange";
}

/** The PIM source id of an Exchange account (`exchange_pim_source_id`). */
export function exchangeSourceId(accountId: string): string {
  return `exchange:${accountId}`;
}

// ---------------------------------------------------------------------------
// Categories

/** Longest category name (`MAX_CATEGORY_NAME`). */
export const MAX_CATEGORY_NAME = 255;

/** Outlook's preset colours (`CategoryColor::rgb`), `preset0` first. */
export const PRESET_RGB: readonly string[] = [
  "#e7a1a2",
  "#f9ba89",
  "#f7dd8f",
  "#fcfa90",
  "#78d168",
  "#9fdcc9",
  "#c6d2b0",
  "#9db7e8",
  "#b5a1e2",
  "#daaec2",
  "#dad9dc",
  "#6b7994",
  "#bfbfbf",
  "#6f6f6f",
  "#4f4f4f",
  "#c11a25",
  "#e2620d",
  "#c79930",
  "#b9b300",
  "#368f2b",
  "#329b7a",
  "#778b45",
  "#2858a5",
  "#5c3fa3",
  "#93446b",
];

/** The colour's RGB (`null` for `none`). */
export function categoryRgb(color: CategoryColor): string | null {
  if (color === "none") return null;
  return PRESET_RGB[Number(color.slice("preset".length))] ?? null;
}

/** A readable text colour on the category colour (dark presets get white text). */
export function categoryTextRgb(color: CategoryColor): string | null {
  const rgb = categoryRgb(color);
  if (rgb === null) return null;
  const n = Number.parseInt(rgb.slice(1), 16);
  const channel = (c: number) => {
    const s = c / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  const luminance =
    0.2126 * channel((n >> 16) & 255) +
    0.7152 * channel((n >> 8) & 255) +
    0.0722 * channel(n & 255);
  // The text with the better contrast (WCAG): black above ~0.18.
  return luminance > 0.179 ? "#000000" : "#ffffff";
}

/** Mirrors `ExchangeCategorySettings::issues` plus duplicates among `others`. */
export function categoryIssues(
  settings: ExchangeCategorySettings,
  others: readonly Pick<ExchangeCategory, "name">[],
): ExchangeCategoryIssue[] {
  const out: ExchangeCategoryIssue[] = [];
  const name = settings.name.trim();
  if (name === "") return ["missing-name"];
  if (charCount(name) > MAX_CATEGORY_NAME) out.push("name-too-long");
  // Outlook separates categories with commas / semicolons.
  // eslint-disable-next-line no-control-regex -- control characters are what it rejects.
  if (/[,;\u0000-\u001f\u007f-\u009f]/.test(name)) out.push("invalid-name");
  if (others.some((c) => c.name.toLowerCase() === name.toLowerCase())) out.push("duplicate-name");
  return out;
}

/** The category of a tag (case-insensitive like Outlook). */
export function categoryOfTag(
  categories: readonly ExchangeCategory[],
  tag: string,
): ExchangeCategory | undefined {
  const lower = tag.toLowerCase();
  return categories.find((c) => c.name.toLowerCase() === lower);
}

/** A tag view: the messages of a category across folders. */
export function tagQuery(tag: string): string {
  return `tag:"${tag.replace(/["\\]/g, "\\$&")}"`;
}

// ---------------------------------------------------------------------------
// Focused Inbox

/** The query of a Focused Inbox tab (`focus:focused` / `focus:other`). */
export function focusQuery(focus: InferenceClassification): string {
  return `focus:${focus}`;
}

/** The tab a query shows, `null` when it is not exactly a Focused Inbox tab. */
export function focusOfQuery(text: string | null | undefined): InferenceClassification | null {
  const match = /^focus:(focused|other)$/.exec((text ?? "").trim());
  return match ? (match[1] as InferenceClassification) : null;
}

// ---------------------------------------------------------------------------
// Mailboxes

/** Mirrors `ExchangeMailboxSettings::issues` (`own` = the account's address) plus duplicates. */
export function mailboxIssues(
  settings: ExchangeMailboxSettings,
  own: string,
  others: readonly Pick<ExchangeMailbox, "settings">[],
): ExchangeMailboxIssue[] {
  const out: ExchangeMailboxIssue[] = [];
  const address = settings.address.trim();
  if (address === "") out.push("missing-address");
  else if (!isPlausibleEmail(address)) out.push("invalid-address");
  else if (address.toLowerCase() === own.trim().toLowerCase()) out.push("own-mailbox");
  else if (others.some((m) => m.settings.address.toLowerCase() === address.toLowerCase())) {
    out.push("duplicate");
  }
  if (settings.name !== null && settings.name.trim() === "") out.push("empty-name");
  if (!(settings.mail || settings.calendar || settings.contacts || settings.tasks)) {
    out.push("nothing-shown");
  }
  return out;
}

/** `nail_api::pim::is_plausible_email`. */
export function isPlausibleEmail(text: string): boolean {
  const at = text.lastIndexOf("@");
  if (at <= 0) return false;
  const domain = text.slice(at + 1);
  return (
    domain !== "" &&
    !domain.startsWith(".") &&
    !domain.endsWith(".") &&
    // eslint-disable-next-line no-control-regex -- control characters are what it rejects.
    !/[\s\u0000-\u001f\u007f]/.test(text)
  );
}

/** The mailbox address of a folder path under another mailbox (`@shared@x/Inbox` → `shared@x`). */
export function mailboxOfPath(path: string): string | null {
  if (!path.startsWith("@")) return null;
  const rest = path.slice(1);
  const slash = rest.indexOf("/");
  const address = (slash === -1 ? rest : rest.slice(0, slash)).toLowerCase();
  return address === "" ? null : address;
}

/** The mailbox a folder belongs to (`null` = the account's own). */
export function mailboxOfFolder(
  folder: Pick<Folder, "path">,
  mailboxes: readonly ExchangeMailbox[],
): ExchangeMailbox | null {
  const address = mailboxOfPath(folder.path);
  if (address === null) return null;
  return mailboxes.find((m) => m.settings.address.toLowerCase() === address) ?? null;
}

/** The name a mailbox is shown with. */
export function mailboxName(mailbox: Pick<ExchangeMailbox, "settings">): string {
  return mailbox.settings.name ?? mailbox.settings.address;
}

/** What a mailbox shows, for its summary (`mail`, `calendar`, …). */
export function mailboxShows(
  settings: ExchangeMailboxSettings,
): ("mail" | "calendar" | "contacts" | "tasks")[] {
  return (["mail", "calendar", "contacts", "tasks"] as const).filter((k) => settings[k]);
}

/** Fresh settings for a new mailbox: shared mailboxes show mail, delegates the calendar too. */
export function newMailboxSettings(): ExchangeMailboxSettings {
  return {
    address: "",
    name: null,
    kind: "shared",
    mail: true,
    calendar: false,
    contacts: false,
    tasks: false,
  };
}

// ---------------------------------------------------------------------------
// Automatic replies

/** Longest reply (`MAX_REPLY_CHARS`). */
export const MAX_REPLY_CHARS = 32_000;

const UTC_TIME = /^\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}Z$/;

/** Mirrors `AutomaticRepliesSettings::issues`. */
export function automaticRepliesIssues(
  settings: AutomaticRepliesSettings,
): AutomaticRepliesIssue[] {
  const out: AutomaticRepliesIssue[] = [];
  const enabled = settings.status !== "disabled";
  if (enabled && settings.internal_reply.trim() === "") out.push("empty-internal-reply");
  if (enabled && settings.external_audience !== "none" && settings.external_reply.trim() === "") {
    out.push("empty-external-reply");
  }
  if (
    charCount(settings.internal_reply) > MAX_REPLY_CHARS ||
    charCount(settings.external_reply) > MAX_REPLY_CHARS
  ) {
    out.push("reply-too-long");
  }
  const { start, end } = settings;
  if ((start !== null && !UTC_TIME.test(start)) || (end !== null && !UTC_TIME.test(end))) {
    out.push("invalid-time");
  } else if (settings.status === "scheduled") {
    if (start === null || end === null) out.push("missing-schedule");
    else if (end <= start) out.push("end-before-start");
  }
  return out;
}

/** Whether replies go out at `now` (always on, or inside the schedule). */
export function repliesActive(settings: AutomaticRepliesSettings, now: number): boolean {
  if (settings.status === "always-enabled") return true;
  if (settings.status !== "scheduled" || settings.start === null || settings.end === null) {
    return false;
  }
  return Date.parse(settings.start) <= now && now < Date.parse(settings.end);
}

/** Whether a schedule is still ahead (on, but not yet sending). */
export function repliesUpcoming(settings: AutomaticRepliesSettings, now: number): boolean {
  return (
    settings.status === "scheduled" && settings.start !== null && now < Date.parse(settings.start)
  );
}

// ---------------------------------------------------------------------------
// Account setup

/** Well-known Microsoft hosts an Autodiscover answer may point to without a warning. */
const MICROSOFT_DOMAINS = ["office365.com", "outlook.office.com", "outlook.com", "microsoft.com"];

const under = (host: string, domain: string) => host === domain || host.endsWith(`.${domain}`);

/** `is_exchange_online_host`. */
export function isExchangeOnlineHost(host: string): boolean {
  const h = host.replace(/\.$/, "").toLowerCase();
  return under(h, "office365.com") || under(h, "outlook.office.com") || under(h, "outlook.com");
}

/** The host of a URL (`null` when it is not one). */
export function urlHost(url: string): string | null {
  try {
    return new URL(url).hostname.replace(/^\[|\]$/g, "").toLowerCase() || null;
  } catch {
    return null;
  }
}

/** The server an Exchange candidate talks to (the EWS URL's host or the server host). */
export function exchangeHost(candidate: Pick<DiscoveredAccount, "incoming">): string {
  const incoming = candidate.incoming;
  if (incoming.protocol === "exchange" && incoming.options.url) {
    return urlHost(incoming.options.url) ?? incoming.server.host;
  }
  return incoming.server.host;
}

/**
 * The host an Autodiscover answer redirected to when it lies outside the
 * address's domain (and is not Microsoft's), else `null`: the user should
 * confirm before sending a password there.
 */
export function redirectedHost(
  email: string,
  candidate: Pick<DiscoveredAccount, "incoming" | "source" | "warnings">,
): string | null {
  if (candidate.incoming.protocol !== "exchange") return null;
  const domain = (email.split("@")[1] ?? "").replace(/\.$/, "").toLowerCase();
  const host = exchangeHost(candidate).replace(/\.$/, "").toLowerCase();
  if (domain === "" || host === "") return null;
  if (candidate.warnings.includes("foreign-hosts")) return host;
  if (under(host, domain)) return null;
  if (MICROSOFT_DOMAINS.some((d) => under(host, d))) return null;
  return host;
}

/** `ExchangeOptions::warnings`. */
export function exchangeWarnings(incoming: DiscoveredAccount["incoming"]): ExchangeWarning[] {
  if (incoming.protocol !== "exchange" || incoming.options.engine !== "ews") return [];
  return isExchangeOnlineHost(exchangeHost({ incoming })) ? ["ews-retired-online"] : [];
}

/** Default options of an `exchange` node for an engine. */
export function exchangeOptions(
  engine: ExchangeEngine,
  url: string | null = null,
): ExchangeOptions {
  return {
    engine,
    url: engine === "ews" ? url : null,
    poll_interval_secs: null,
    max_poll_interval_secs: null,
    streaming: null,
    server_rules: null,
  };
}

/** An Exchange setup candidate without discovery (the user chose Exchange). */
export function exchangeCandidate(
  email: string,
  engine: ExchangeEngine,
  host: string | null = null,
): DiscoveredAccount {
  const domain = email.split("@")[1] ?? "";
  const auth: AuthConfig =
    engine === "graph"
      ? { method: "oauth2", username: email, provider: "microsoft", secret: null, mechanism: null }
      : { method: "windows", username: null, secret: null, package: null };
  return {
    source: "guess",
    provider_name: null,
    incoming: {
      protocol: "exchange",
      server: {
        host:
          engine === "graph" ? "graph.microsoft.com" : (host ?? (domain ? `mail.${domain}` : "")),
        port: 443,
        tls: "implicit",
        auth,
      },
      options: exchangeOptions(engine),
    },
    outgoing: null,
    warnings: [],
  };
}

/** Switches a candidate's engine, with the sign-in that engine needs. */
export function withEngine(
  candidate: DiscoveredAccount,
  engine: ExchangeEngine,
  email: string,
): DiscoveredAccount {
  const incoming = candidate.incoming;
  if (incoming.protocol !== "exchange" || incoming.options.engine === engine) return candidate;
  const fresh = exchangeCandidate(email, engine);
  const host =
    engine === "graph"
      ? "graph.microsoft.com"
      : incoming.server.host === "graph.microsoft.com"
        ? fresh.incoming.server.host
        : incoming.server.host;
  return {
    ...candidate,
    incoming: {
      protocol: "exchange",
      server: { ...incoming.server, host, auth: fresh.incoming.server.auth },
      options: { ...exchangeOptions(engine), server_rules: incoming.options.server_rules },
    },
  };
}

/** How an Exchange account signs in. */
export type ExchangeSignIn = "windows-current" | "windows-account" | "password" | "oauth2";

/** The sign-in methods an engine accepts (Graph: OAuth only). */
export function exchangeSignIns(engine: ExchangeEngine): readonly ExchangeSignIn[] {
  return engine === "graph"
    ? ["oauth2"]
    : ["windows-current", "windows-account", "password", "oauth2"];
}

export function exchangeSignInOf(auth: AuthConfig): ExchangeSignIn {
  if (auth.method === "windows") return auth.username ? "windows-account" : "windows-current";
  if (auth.method === "oauth2") return "oauth2";
  return "password";
}

/** Applies a sign-in method to the Exchange server's auth (`email` = default login name). */
export function withExchangeSignIn(
  auth: AuthConfig,
  signIn: ExchangeSignIn,
  email: string,
): AuthConfig {
  const login =
    auth.method === "password" || auth.method === "oauth2"
      ? auth.username
      : auth.method === "windows"
        ? (auth.username ?? "")
        : "";
  const pkg: SspiPackage | null = auth.method === "windows" ? auth.package : null;
  switch (signIn) {
    case "windows-current":
      return { method: "windows", username: null, secret: null, package: pkg };
    case "windows-account":
      return {
        method: "windows",
        username: login.includes("@") || login === "" ? defaultWindowsLogin(email) : login,
        secret: null,
        package: pkg,
      };
    case "password":
      return { method: "password", username: login || email, secret: null, mechanism: null };
    case "oauth2":
      return {
        method: "oauth2",
        username: email || login,
        provider: "microsoft",
        secret: null,
        mechanism: null,
      };
  }
}

/** A starting `DOMAIN\user` login from the address (`ada@corp.example` → `CORP\ada`). */
export function defaultWindowsLogin(email: string): string {
  const [local = "", domain = ""] = email.split("@");
  const first = domain.split(".")[0] ?? "";
  return first ? `${first.toUpperCase()}\\${local}` : local;
}

/** Whether the sign-in has what it needs (the current Windows user needs nothing). */
export function exchangeCredentialsReady(
  auth: AuthConfig,
  password: string,
  oauthDone: boolean,
): boolean {
  switch (exchangeSignInOf(auth)) {
    case "windows-current":
      return true;
    case "oauth2":
      return oauthDone;
    default:
      return password !== "";
  }
}

/** Secrets to hand to the core (an Exchange account has only the incoming endpoint). */
export function exchangeSecretsFor(
  config: Pick<AccountConfig, "incoming">,
  password: string,
  flowId: string | null,
): SecretInput[] {
  const signIn = exchangeSignInOf(config.incoming.server.auth);
  if (signIn === "windows-current") return [];
  if (signIn === "oauth2") {
    return flowId === null
      ? []
      : [{ endpoint: "incoming", value: { kind: "oauth2", flow_id: flowId } }];
  }
  return password === "" ? [] : [{ endpoint: "incoming", value: { kind: "password", password } }];
}

// ---------------------------------------------------------------------------
// Rooms

/** Whether an attendee is a room. */
export function isRoom(attendee: Pick<Attendee, "kind">): boolean {
  return attendee.kind === "room";
}

/** A room as an attendee (books it). */
export function roomAttendee(room: { address: string; name: string }): Attendee {
  return {
    email: room.address,
    name: room.name,
    role: "non-participant",
    status: "needs-action",
    kind: "room",
    rsvp: true,
    delegated_to: [],
    delegated_from: [],
  };
}

/** Adds a room (once) to the attendees. */
export function addRoom(
  attendees: readonly Attendee[],
  room: { address: string; name: string },
): Attendee[] {
  const lower = room.address.toLowerCase();
  if (attendees.some((a) => a.email.toLowerCase() === lower)) return [...attendees];
  return [...attendees, roomAttendee(room)];
}

/** Removes a room from the attendees. */
export function removeRoom(attendees: readonly Attendee[], address: string): Attendee[] {
  const lower = address.toLowerCase();
  return attendees.filter((a) => a.email.toLowerCase() !== lower);
}

/** Free / busy / unknown of a room in the asked range. */
export function roomState(availability: RoomAvailability): "free" | "busy" | "unknown" {
  if (availability.error !== null || availability.free === null) return "unknown";
  return availability.free ? "free" : "busy";
}

/** Busy periods overlapping `[from, to)`. */
export function overlapping(
  periods: readonly BusyPeriod[],
  from: string,
  to: string,
): BusyPeriod[] {
  const a = Date.parse(from);
  const b = Date.parse(to);
  return periods.filter((p) => Date.parse(p.start) < b && Date.parse(p.end) > a);
}

// ---------------------------------------------------------------------------
// Task lists

export type TaskListGroupKind = "todo" | "mailbox" | "other";

export interface TaskListGroup {
  kind: TaskListGroupKind;
  /** Account id (`todo`) or mailbox address (`mailbox`); `null` for `other`. */
  key: string | null;
  lists: Calendar[];
}

/** Task lists grouped: each account's To Do lists, other mailboxes' lists, then the rest. */
export function groupTaskLists(lists: readonly Calendar[]): TaskListGroup[] {
  const groups: TaskListGroup[] = [];
  const groupFor = (kind: TaskListGroupKind, key: string | null) => {
    let group = groups.find((g) => g.kind === kind && g.key === key);
    if (!group) {
      group = { kind, key, lists: [] };
      groups.push(group);
    }
    return group;
  };
  for (const list of lists) {
    const info = list.exchange;
    if (info?.mailbox) groupFor("mailbox", info.mailbox).lists.push(list);
    else if (info) groupFor("todo", list.account_id ?? list.source_id).lists.push(list);
    else groupFor("other", null).lists.push(list);
  }
  const rank = (list: Calendar) => {
    const role = list.exchange?.role;
    return role === "default-tasks" ? 0 : role === "flagged-emails" ? 2 : 1;
  };
  for (const g of groups) {
    g.lists.sort(
      (a, b) =>
        rank(a) - rank(b) ||
        a.settings.order - b.settings.order ||
        a.settings.name.localeCompare(b.settings.name),
    );
  }
  const order: Record<TaskListGroupKind, number> = { todo: 0, mailbox: 1, other: 2 };
  return groups.sort((a, b) => order[a.kind] - order[b.kind]);
}
