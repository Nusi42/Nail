/**
 * A demo Exchange Online account for the mock core (`?mock=1`, tests): the
 * account in the accounts list and the `accounts.kdl` model (an `exchange`
 * node, Graph, Microsoft OAuth), its folders (well-known ones plus the
 * automapped shared mailbox `support@` under `@support@contoso.example`),
 * inbox mail sorted into Focused / Other with categories (Nail tags), the
 * category master list with Outlook colours, other mailboxes (shared,
 * delegate calendar, one without access), a directory of mailboxes that
 * can be added, a placed rule and the user's own server rules, automatic
 * replies, room lists and rooms with busy windows, the status, and the
 * account's PIM source `exchange:acc-exchange` with its calendar, the
 * boss's delegate calendar and Microsoft To Do lists with tasks.
 *
 * `seedExchangeDemo(data)` is called once on fresh `MockData` (the
 * integrator calls it from `createMockData`; tests call it themselves).
 */
import type {
  Calendar,
  CategoryColor,
  FolderRole,
  Identity,
  InferenceClassification,
  MessageSummary,
  Task,
} from "../../api/generated";
import type { MockData } from "../../api/mockData";
import { emptyTaskFields } from "../pim/defaults";
import { emptyActions, emptyConditions } from "./mock";
import { exchangeSourceId } from "./model";

export const EXCHANGE_ACCOUNT = "acc-exchange";
export const EXCHANGE_EMAIL = "ada@contoso.example";
export const SHARED_MAILBOX = "support@contoso.example";
export const DELEGATE_MAILBOX = "charles@contoso.example";
export const DENIED_MAILBOX = "legal@contoso.example";
/** In the directory, not open yet (tests add it). */
export const ADDABLE_MAILBOX = "sales@contoso.example";

/** Folder ids of the demo account by role. */
export const EXCHANGE_FOLDERS: Record<
  "inbox" | "sent" | "drafts" | "archive" | "junk" | "trash",
  string
> = {
  inbox: `${EXCHANGE_ACCOUNT}/Inbox`,
  sent: `${EXCHANGE_ACCOUNT}/Sent Items`,
  drafts: `${EXCHANGE_ACCOUNT}/Drafts`,
  archive: `${EXCHANGE_ACCOUNT}/Archive`,
  junk: `${EXCHANGE_ACCOUNT}/Junk Email`,
  trash: `${EXCHANGE_ACCOUNT}/Deleted Items`,
};

/** The shared mailbox's root and inbox folder ids. */
export const SHARED_ROOT = `${EXCHANGE_ACCOUNT}/@${SHARED_MAILBOX}`;
export const SHARED_INBOX = `${SHARED_ROOT}/Inbox`;

/** Calendar ids of the Exchange PIM source. */
export const EXCHANGE_CALENDARS = {
  calendar: "exchange-cal-calendar",
  boss: "exchange-cal-charles",
  tasks: "exchange-todo-tasks",
  groceries: "exchange-todo-groceries",
  flagged: "exchange-todo-flagged",
} as const;

const FOLDER_NAMES: [keyof typeof EXCHANGE_FOLDERS, string][] = [
  ["inbox", "Inbox"],
  ["sent", "Sent Items"],
  ["drafts", "Drafts"],
  ["archive", "Archive"],
  ["junk", "Junk Email"],
  ["trash", "Deleted Items"],
];

interface DemoMail {
  subject: string;
  from: { name: string | null; email: string };
  folder: string;
  focus: InferenceClassification | null;
  tags: string[];
  seen: boolean;
  mailbox?: string;
}

const MAILS: DemoMail[] = [
  {
    subject: "Phoenix launch checklist",
    from: { name: "Grace Hopper", email: "grace@contoso.example" },
    folder: EXCHANGE_FOLDERS.inbox,
    focus: "focused",
    tags: ["Project Phoenix", "Urgent"],
    seen: false,
  },
  {
    subject: "Quarterly numbers",
    from: { name: "Charles Babbage", email: DELEGATE_MAILBOX },
    folder: EXCHANGE_FOLDERS.inbox,
    focus: "focused",
    tags: ["Project Phoenix"],
    seen: true,
  },
  {
    subject: "Contract renewal for Fabrikam",
    from: { name: "Fabrikam Purchasing", email: "purchasing@fabrikam.example" },
    folder: EXCHANGE_FOLDERS.inbox,
    focus: "focused",
    tags: ["customers"],
    seen: false,
  },
  {
    subject: "Your weekly digest",
    from: { name: "Newsletter", email: "news@digest.example" },
    folder: EXCHANGE_FOLDERS.inbox,
    focus: "other",
    tags: [],
    seen: false,
  },
  {
    subject: "50% off conference passes",
    from: { name: "Events", email: "promo@events.example" },
    folder: EXCHANGE_FOLDERS.inbox,
    focus: "other",
    tags: [],
    seen: true,
  },
  {
    subject: "Dentist appointment reminder",
    from: { name: "Dental clinic", email: "noreply@clinic.example" },
    folder: EXCHANGE_FOLDERS.archive,
    focus: null,
    tags: ["Personal"],
    seen: true,
  },
  {
    subject: "Printer on floor 3 is broken",
    from: { name: "Alan Turing", email: "alan@contoso.example" },
    folder: SHARED_INBOX,
    focus: null,
    tags: [],
    seen: false,
    mailbox: SHARED_MAILBOX,
  },
  {
    subject: "Password reset request",
    from: { name: "Katherine Johnson", email: "katherine@contoso.example" },
    folder: SHARED_INBOX,
    focus: null,
    tags: ["Urgent"],
    seen: true,
    mailbox: SHARED_MAILBOX,
  },
];

const CATEGORIES: [string, string, CategoryColor][] = [
  ["{0001-cat}", "Customers", "preset4"],
  ["{0002-cat}", "Personal", "none"],
  ["{0003-cat}", "Project Phoenix", "preset7"],
  ["{0004-cat}", "Urgent", "preset0"],
];

function calendar(
  id: string,
  name: string,
  color: string,
  components: Calendar["components"],
  exchange: NonNullable<Calendar["exchange"]>,
  order: number,
  readOnly = false,
): Calendar {
  return {
    id,
    source_id: exchangeSourceId(EXCHANGE_ACCOUNT),
    account_id: EXCHANGE_ACCOUNT,
    settings: {
      name,
      description: null,
      color,
      visible: true,
      reminders: true,
      default_alarms: [],
      order,
    },
    components,
    read_only: readOnly,
    timezone: "Europe/Berlin",
    exchange,
  };
}

function task(id: string, list: string, title: string, extra: Partial<Task["fields"]> = {}): Task {
  return {
    id,
    calendar_id: list,
    uid: `${id}@contoso.example`,
    fields: { ...emptyTaskFields(title), ...extra },
    recurrence: null,
    subtask_ids: [],
    created_at: null,
    updated_at: null,
    read_only: false,
    sync: "synced",
  };
}

const iso = (ms: number) => new Date(ms).toISOString().replace(/\.\d{3}Z$/, "Z");

/** Adds the demo Exchange account to fresh mock data. */
export function seedExchangeDemo(data: MockData, options: { now?: number } = {}): void {
  const now = options.now ?? Date.parse("2026-09-29T08:00:00Z");
  data.accounts.push({
    id: EXCHANGE_ACCOUNT,
    name: "Contoso",
    email: EXCHANGE_EMAIL,
    kind: "exchange",
  });
  const identity: Identity = {
    id: EXCHANGE_EMAIL,
    account_id: EXCHANGE_ACCOUNT,
    email: EXCHANGE_EMAIL,
    name: "Ada Lovelace",
    reply_to: null,
    bcc: [],
    signature: null,
    composer_mode: null,
    posting: null,
    mail_style: null,
  };
  data.accountsConfig.accounts.push({
    id: EXCHANGE_ACCOUNT,
    name: "Contoso",
    email: EXCHANGE_EMAIL,
    enabled: true,
    incoming: {
      protocol: "exchange",
      server: {
        host: "graph.microsoft.com",
        port: 443,
        tls: "implicit",
        auth: {
          method: "oauth2",
          username: EXCHANGE_EMAIL,
          provider: "microsoft",
          secret: null,
          mechanism: null,
        },
      },
      options: {
        engine: "graph",
        url: null,
        poll_interval_secs: null,
        max_poll_interval_secs: null,
        streaming: null,
        server_rules: null,
      },
    },
    outgoing: null,
    identities: [identity],
    sync: { bodies: null, folders: [] },
    save_sent_copy: null,
  });
  data.statuses.push({
    account_id: EXCHANGE_ACCOUNT,
    state: "online",
    since: iso(now),
    error: null,
    message: null,
  });

  for (const [role, name] of FOLDER_NAMES) {
    data.folders.push({
      id: EXCHANGE_FOLDERS[role],
      account_id: EXCHANGE_ACCOUNT,
      parent_id: null,
      name,
      path: name,
      role: role as FolderRole,
      unread: 0,
      total: 0,
    });
  }
  data.folders.push(
    {
      id: SHARED_ROOT,
      account_id: EXCHANGE_ACCOUNT,
      parent_id: null,
      name: "Support",
      path: `@${SHARED_MAILBOX}`,
      role: "other",
      unread: 0,
      total: 0,
    },
    {
      id: SHARED_INBOX,
      account_id: EXCHANGE_ACCOUNT,
      parent_id: SHARED_ROOT,
      name: "Inbox",
      path: `@${SHARED_MAILBOX}/Inbox`,
      role: "other",
      unread: 0,
      total: 0,
    },
  );

  MAILS.forEach((mail, index) => {
    const id = `${mail.folder}#x${index + 1}`;
    const itemId = `AAMkAD${(0x2000 + index).toString(16)}`;
    const summary: MessageSummary = {
      id,
      folder_id: mail.folder,
      account_id: EXCHANGE_ACCOUNT,
      subject: mail.subject,
      from: mail.from,
      to: [{ name: "Ada Lovelace", email: mail.mailbox ?? EXCHANGE_EMAIL }],
      date: iso(now - index * 47 * 60_000),
      flags: { seen: mail.seen, flagged: false, answered: false, draft: false },
      has_attachments: false,
      preview: mail.subject,
      thread_id: `xconv-${itemId}`,
      tags: [...mail.tags].sort(),
      duplicates: [],
      exchange: {
        item_id: itemId,
        conversation_id: `conv-${itemId}`,
        inference: mail.focus,
        mailbox: mail.mailbox ?? null,
      },
    };
    data.messages.push(summary);
    data.bodies.set(id, {
      id,
      headers: {
        subject: mail.subject,
        from: [mail.from],
        to: summary.to,
        cc: [],
        reply_to: [],
        date: summary.date,
        message_id: `${itemId}@contoso.example`,
      },
      text: `${mail.subject}\n\n(mock Exchange message)`,
      sanitized_html: null,
      attachments: [],
      remote_content_blocked: false,
    });
    const folder = data.folders.find((f) => f.id === mail.folder);
    if (folder) {
      folder.total++;
      if (!mail.seen) folder.unread++;
    }
  });

  const state = data.features.exchange;
  state.categories[EXCHANGE_ACCOUNT] = CATEGORIES.map(([id, name, color]) => ({
    account_id: EXCHANGE_ACCOUNT,
    id,
    name,
    color,
    total: 0,
    unread: 0,
    sync: "synced",
  }));

  state.directory = {
    [SHARED_MAILBOX]: { name: "Support", access: true, folders: ["Inbox"] },
    [DELEGATE_MAILBOX]: { name: "Charles Babbage", access: true, folders: ["Inbox"] },
    [DENIED_MAILBOX]: { name: "Legal", access: false, folders: [] },
    [ADDABLE_MAILBOX]: { name: "Sales", access: true, folders: ["Inbox", "Leads"] },
  };
  state.mailboxes[EXCHANGE_ACCOUNT] = [
    {
      account_id: EXCHANGE_ACCOUNT,
      settings: {
        address: SHARED_MAILBOX,
        name: "Support",
        kind: "shared",
        mail: true,
        calendar: false,
        contacts: false,
        tasks: false,
      },
      origin: "automapped",
      state: "synced",
      root_folder_id: SHARED_ROOT,
      calendar_ids: [],
      address_book_ids: [],
      error: null,
    },
    {
      account_id: EXCHANGE_ACCOUNT,
      settings: {
        address: DELEGATE_MAILBOX,
        name: "Charles Babbage",
        kind: "delegate",
        mail: false,
        calendar: true,
        contacts: false,
        tasks: false,
      },
      origin: "user",
      state: "synced",
      root_folder_id: null,
      calendar_ids: [EXCHANGE_CALENDARS.boss],
      address_book_ids: [],
      error: null,
    },
    {
      account_id: EXCHANGE_ACCOUNT,
      settings: {
        address: DENIED_MAILBOX,
        name: null,
        kind: "shared",
        mail: true,
        calendar: false,
        contacts: false,
        tasks: false,
      },
      origin: "user",
      state: "access-denied",
      root_folder_id: null,
      calendar_ids: [],
      address_book_ids: [],
      error: "forbidden",
      localized: { id: "exchange-mailbox-state-access-denied" },
    },
  ];

  state.automaticReplies[EXCHANGE_ACCOUNT] = {
    account_id: EXCHANGE_ACCOUNT,
    settings: {
      status: "disabled",
      start: null,
      end: null,
      internal_reply: "I am out of the office. Grace Hopper stands in for me.",
      external_reply: "Thank you for your message. I will answer when I am back.",
      external_audience: "contacts-only",
    },
    sync: "synced",
    fetched_at: iso(now - 3_600_000),
  };

  state.rules[EXCHANGE_ACCOUNT] = {
    account_id: EXCHANGE_ACCOUNT,
    state: "in-sync",
    rules: [],
    server_rules: [
      {
        id: "inbox-rule-1",
        name: "Rust announcements",
        sequence: 1,
        enabled: true,
        conditions: { ...emptyConditions(), subject_contains: ["[rust-announce]"] },
        exceptions: emptyConditions(),
        actions: { ...emptyActions(), assign_categories: ["rust"] },
        has_unsupported: false,
        in_error: false,
        nail_rule_id: "rust-announcements",
      },
      {
        id: "inbox-rule-2",
        name: "Invoices to accounting",
        sequence: 2,
        enabled: true,
        conditions: { ...emptyConditions(), subject_contains: ["Invoice"] },
        exceptions: emptyConditions(),
        actions: { ...emptyActions(), forward_to: ["accounting@contoso.example"] },
        has_unsupported: false,
        in_error: false,
        nail_rule_id: null,
      },
      {
        id: "inbox-rule-3",
        name: "Move newsletters (created in Outlook)",
        sequence: 3,
        enabled: false,
        conditions: emptyConditions(),
        exceptions: emptyConditions(),
        actions: { ...emptyActions(), move_to_folder: EXCHANGE_FOLDERS.archive },
        has_unsupported: true,
        in_error: false,
        nail_rule_id: null,
      },
    ],
    synced_at: iso(now - 600_000),
    error: null,
  };

  state.roomLists[EXCHANGE_ACCOUNT] = [
    { address: "building1@contoso.example", name: "Building 1" },
    { address: "building2@contoso.example", name: "Building 2" },
  ];
  state.rooms[EXCHANGE_ACCOUNT] = [
    {
      address: "room-ada@contoso.example",
      name: "Ada room",
      capacity: 8,
      building: "Building 1",
      floor: "2",
      accessible: true,
      features: ["display", "video"],
      room_list: "building1@contoso.example",
    },
    {
      address: "room-babbage@contoso.example",
      name: "Babbage hall",
      capacity: 40,
      building: "Building 1",
      floor: "0",
      accessible: true,
      features: ["projector", "microphones"],
      room_list: "building1@contoso.example",
    },
    {
      address: "room-turing@contoso.example",
      name: "Turing room",
      capacity: 4,
      building: "Building 2",
      floor: "5",
      accessible: false,
      features: [],
      room_list: "building2@contoso.example",
    },
    {
      address: "room-hopper@contoso.example",
      name: "Hopper lab",
      capacity: 12,
      building: "Building 2",
      floor: "1",
      accessible: null,
      features: ["whiteboard"],
      room_list: "building2@contoso.example",
    },
  ];
  state.roomBusy = {
    // Busy every day 08:00–10:00 UTC.
    "room-ada@contoso.example": [{ start: 8, end: 10 }],
    // Busy all day.
    "room-babbage@contoso.example": [{ start: 0, end: 24 }],
    // Always free.
    "room-turing@contoso.example": [],
    // `room-hopper` has no entry: its availability cannot be read.
  };

  state.status[EXCHANGE_ACCOUNT] = {
    account_id: EXCHANGE_ACCOUNT,
    engine: "graph",
    server_version: null,
    push: "polling",
    last_sync_at: iso(now - 120_000),
    throttled_until: null,
    folders_synced: 8,
    folders_total: 8,
    pending_ops: 0,
    warnings: [],
  };

  // The account's PIM source with its calendar, the delegate calendar and To Do lists.
  const pim = data.features.pim;
  pim.sources.push({
    id: exchangeSourceId(EXCHANGE_ACCOUNT),
    kind: "exchange",
    name: "Contoso",
    account_id: EXCHANGE_ACCOUNT,
    enabled: true,
    config: null,
    state: "idle",
    last_sync_at: iso(now - 120_000),
    error: null,
    message: null,
  });
  pim.calendars.push(
    calendar(
      EXCHANGE_CALENDARS.calendar,
      "Calendar",
      "#2858a5",
      ["event"],
      { mailbox: null, role: "default-calendar", todo: false },
      10,
    ),
    calendar(
      EXCHANGE_CALENDARS.boss,
      "Charles Babbage — Calendar",
      "#93446b",
      ["event"],
      { mailbox: DELEGATE_MAILBOX, role: "default-calendar", todo: false },
      11,
    ),
    calendar(
      EXCHANGE_CALENDARS.tasks,
      "Tasks",
      "#368f2b",
      ["task"],
      { mailbox: null, role: "default-tasks", todo: true },
      12,
    ),
    calendar(
      EXCHANGE_CALENDARS.groceries,
      "Groceries",
      "#e2620d",
      ["task"],
      { mailbox: null, role: null, todo: true },
      13,
    ),
    calendar(
      EXCHANGE_CALENDARS.flagged,
      "Flagged email",
      "#c11a25",
      ["task"],
      { mailbox: null, role: "flagged-emails", todo: true },
      14,
      true,
    ),
  );
  pim.tasks.push(
    task("exchange-task-1", EXCHANGE_CALENDARS.tasks, "Prepare the Phoenix launch", {
      due: { type: "date", date: iso(now + 2 * 86_400_000).slice(0, 10) },
      priority: 1,
    }),
    task("exchange-task-2", EXCHANGE_CALENDARS.groceries, "Coffee beans"),
    task("exchange-task-3", EXCHANGE_CALENDARS.flagged, "Contract renewal for Fabrikam"),
  );
}
