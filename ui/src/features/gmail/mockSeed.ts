/**
 * A demo Gmail account for the mock core (`?mock=1`, tests): the account
 * in the accounts list and `accounts.kdl` model (a `gmail` node with
 * OAuth2, no SMTP), its folders (Gmail's system labels shown as folders),
 * messages with categories and user labels (Nail tags), and its labels —
 * nested ones, coloured ones from Gmail's palette, a hidden one and one
 * shown only while it has unread mail.
 *
 * `seedGmailDemo(data)` is called once on fresh `MockData` (the integrator
 * calls it from `createMockData`; tests call it themselves).
 */
import type { GmailCategory, GmailLabel, Identity, MessageSummary } from "../../api/generated";
import type { MockData } from "../../api/mockData";
import { categoryLabel, CATEGORIES } from "./model";

export const GMAIL_ACCOUNT = "acc-gmail";
export const GMAIL_EMAIL = "ada.lovelace@gmail.example";

/** Folder ids of the demo account by role. */
export const GMAIL_FOLDERS: Record<
  "inbox" | "sent" | "drafts" | "archive" | "junk" | "trash",
  string
> = {
  inbox: `${GMAIL_ACCOUNT}/INBOX`,
  sent: `${GMAIL_ACCOUNT}/SENT`,
  drafts: `${GMAIL_ACCOUNT}/DRAFT`,
  archive: `${GMAIL_ACCOUNT}/ALL`,
  junk: `${GMAIL_ACCOUNT}/SPAM`,
  trash: `${GMAIL_ACCOUNT}/TRASH`,
};

const FOLDER_NAMES: [keyof typeof GMAIL_FOLDERS, string, string | null][] = [
  ["inbox", "Inbox", "INBOX"],
  ["sent", "Sent", "SENT"],
  ["drafts", "Drafts", "DRAFT"],
  ["archive", "All Mail", null],
  ["junk", "Spam", "SPAM"],
  ["trash", "Trash", "TRASH"],
];

function label(partial: Partial<GmailLabel> & Pick<GmailLabel, "id" | "name">): GmailLabel {
  return {
    account_id: GMAIL_ACCOUNT,
    kind: "user",
    color: null,
    list_visibility: "show",
    message_visibility: "show",
    tag: partial.kind === "system" ? null : partial.name,
    folder_id: null,
    total: 0,
    unread: 0,
    sync: "synced",
    ...partial,
  };
}

interface DemoMail {
  subject: string;
  from: { name: string | null; email: string };
  folder: keyof typeof GMAIL_FOLDERS;
  category: GmailCategory | null;
  tags: string[];
  seen: boolean;
}

const MAILS: DemoMail[] = [
  {
    subject: "Your train tickets to Lisbon",
    from: { name: "Rail", email: "tickets@rail.example" },
    folder: "inbox",
    category: "updates",
    tags: ["Travel"],
    seen: false,
  },
  {
    subject: "Receipt for your order #4711",
    from: { name: "Shop", email: "orders@shop.example" },
    folder: "inbox",
    category: "updates",
    tags: ["Receipts"],
    seen: true,
  },
  {
    subject: "Grace shared a photo",
    from: { name: "Social", email: "notify@social.example" },
    folder: "inbox",
    category: "social",
    tags: [],
    seen: false,
  },
  {
    subject: "Autumn sale: 30% off",
    from: { name: "Shop", email: "news@shop.example" },
    folder: "inbox",
    category: "promotions",
    tags: [],
    seen: false,
  },
  {
    subject: "Re: Analytical engine notes",
    from: { name: "Charles Babbage", email: "charles@engine.example" },
    folder: "inbox",
    category: "primary",
    tags: ["Work/Projects"],
    seen: false,
  },
  {
    subject: "Lunch on Friday?",
    from: { name: "Mary Somerville", email: "mary@science.example" },
    folder: "inbox",
    category: "primary",
    tags: [],
    seen: true,
  },
  {
    subject: "[rust-users] Weekly digest",
    from: { name: "Rust Users", email: "digest@forum.example" },
    folder: "inbox",
    category: "forums",
    tags: [],
    seen: true,
  },
  {
    subject: "Please review the draft",
    from: { name: "Charles Babbage", email: "charles@engine.example" },
    folder: "inbox",
    category: "primary",
    tags: ["Follow up", "Work"],
    seen: false,
  },
  {
    subject: "Hotel confirmation",
    from: { name: "Hotel", email: "stay@hotel.example" },
    folder: "archive",
    category: "updates",
    tags: ["Travel", "Receipts"],
    seen: true,
  },
  {
    subject: "Old newsletter",
    from: { name: "Weekly", email: "weekly@news.example" },
    folder: "archive",
    category: "promotions",
    tags: ["Old newsletters"],
    seen: true,
  },
  {
    subject: "Notes for Charles",
    from: { name: "Ada Lovelace", email: GMAIL_EMAIL },
    folder: "sent",
    category: null,
    tags: ["Work/Projects"],
    seen: true,
  },
];

/** Adds the demo Gmail account to fresh mock data. */
export function seedGmailDemo(data: MockData, options: { now?: number } = {}): void {
  const now = options.now ?? Date.parse("2026-09-29T08:00:00Z");
  data.accounts.push({ id: GMAIL_ACCOUNT, name: "Gmail", email: GMAIL_EMAIL, kind: "gmail" });
  const identity: Identity = {
    id: GMAIL_EMAIL,
    account_id: GMAIL_ACCOUNT,
    email: GMAIL_EMAIL,
    name: "Ada Lovelace",
    reply_to: null,
    bcc: [],
    signature: null,
    composer_mode: null,
    posting: null,
    mail_style: null,
  };
  data.accountsConfig.accounts.push({
    id: GMAIL_ACCOUNT,
    name: "Gmail",
    email: GMAIL_EMAIL,
    enabled: true,
    incoming: {
      protocol: "gmail",
      server: {
        host: "gmail.googleapis.com",
        port: 443,
        tls: "implicit",
        auth: {
          method: "oauth2",
          username: GMAIL_EMAIL,
          provider: "google",
          secret: null,
          mechanism: null,
        },
      },
      options: { poll_interval_secs: null, max_poll_interval_secs: null },
    },
    outgoing: null,
    identities: [identity],
    sync: { bodies: null, folders: [] },
    save_sent_copy: null,
  });
  data.statuses.push({
    account_id: GMAIL_ACCOUNT,
    state: "online",
    since: new Date(now).toISOString(),
    error: null,
    message: null,
  });

  for (const [role, name] of FOLDER_NAMES) {
    data.folders.push({
      id: GMAIL_FOLDERS[role],
      account_id: GMAIL_ACCOUNT,
      parent_id: null,
      name,
      path: name,
      role: role,
      unread: 0,
      total: 0,
    });
  }

  MAILS.forEach((mail, index) => {
    const folderId = GMAIL_FOLDERS[mail.folder];
    const id = `${folderId}#${String(index + 1)}`;
    const gmailId = `18f${(0x1000 + index).toString(16)}`;
    const system = FOLDER_NAMES.find(([role]) => role === mail.folder)?.[2];
    const summary: MessageSummary = {
      id,
      folder_id: folderId,
      account_id: GMAIL_ACCOUNT,
      subject: mail.subject,
      from: mail.from,
      to: [{ name: "Ada Lovelace", email: GMAIL_EMAIL }],
      date: new Date(now - index * 53 * 60_000).toISOString(),
      flags: { seen: mail.seen, flagged: false, answered: false, draft: false },
      has_attachments: false,
      preview: mail.subject,
      thread_id: `gthread-${gmailId}`,
      tags: [...mail.tags].sort(),
      duplicates: [],
      gmail: {
        gmail_id: gmailId,
        thread_id: `gthread-${gmailId}`,
        label_ids: [
          ...(system ? [system] : []),
          ...(mail.seen ? [] : ["UNREAD"]),
          ...(mail.category ? [categoryLabel(mail.category)] : []),
        ],
        category: mail.category,
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
        message_id: `${gmailId}@mail.gmail.example`,
      },
      text: `${mail.subject}\n\n(mock Gmail message)`,
      sanitized_html: null,
      attachments: [],
      remote_content_blocked: false,
    });
    const folder = data.folders.find((f) => f.id === folderId);
    if (folder) {
      folder.total++;
      if (!mail.seen) folder.unread++;
    }
  });

  const labels: GmailLabel[] = [
    ...FOLDER_NAMES.flatMap(([role, , system]) =>
      system
        ? [label({ id: system, name: system, kind: "system", folder_id: GMAIL_FOLDERS[role] })]
        : [],
    ),
    ...CATEGORIES.map((c) =>
      label({
        id: categoryLabel(c),
        name: categoryLabel(c),
        kind: "system",
        list_visibility: "hide",
      }),
    ),
    label({ id: "Label_1", name: "Receipts", color: { background: "#16a766", text: "#ffffff" } }),
    label({ id: "Label_2", name: "Travel", color: { background: "#4a86e8", text: "#ffffff" } }),
    label({ id: "Label_3", name: "Work" }),
    label({
      id: "Label_4",
      name: "Work/Projects",
      color: { background: "#e4d7f5", text: "#41236d" },
    }),
    label({ id: "Label_5", name: "Old newsletters", list_visibility: "hide" }),
    label({
      id: "Label_6",
      name: "Follow up",
      list_visibility: "show-if-unread",
      color: { background: "#fb4c2f", text: "#ffffff" },
    }),
  ];
  data.features.gmail.labels[GMAIL_ACCOUNT] = labels;
}
