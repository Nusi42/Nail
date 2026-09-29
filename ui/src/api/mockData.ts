/**
 * Deterministic in-memory mail data for `MockTransport` (tests and `?mock=1`).
 * The theme and layout are the reference fixtures from `nail-api`.
 */
import defaultLayoutJson from "../../../crates/nail-api/fixtures/default-layout.json";
import defaultThemeJson from "../../../crates/nail-api/fixtures/default-theme.json";
import type {
  Account,
  Address,
  Folder,
  FolderRole,
  MessageBody,
  MessageSummary,
  ResolvedLayout,
  ResolvedTheme,
} from "./generated";
import { normalizeLayout, normalizeTheme } from "./normalize";

export interface MockData {
  accounts: Account[];
  folders: Folder[];
  messages: MessageSummary[];
  bodies: Map<string, MessageBody>;
  theme: ResolvedTheme;
  layout: ResolvedLayout;
  locale: string | null;
}

export const defaultTheme = (): ResolvedTheme => normalizeTheme(defaultThemeJson);
export const defaultLayout = (): ResolvedLayout => normalizeLayout(defaultLayoutJson);

const PEOPLE: Address[] = [
  { name: "Grace Hopper", email: "grace@navy.example" },
  { name: "Alan Turing", email: "alan@bletchley.example" },
  { name: "Katherine Johnson", email: "katherine@nasa.example" },
  { name: null, email: "noreply@shop.example" },
  { name: "Edsger Dijkstra", email: "ewd@tue.example" },
  { name: "Barbara Liskov", email: "liskov@mit.example" },
];

const SUBJECTS = [
  "Compiler notes for Monday",
  "Re: The bombe schedule",
  "Trajectory numbers",
  "Your order has shipped",
  "Go To statement considered harmful",
  "Abstraction and substitution",
  "Lunch?",
  "Quarterly report draft",
];

/** Base time of the generated mailbox: newest message. */
export const MOCK_NOW = Date.parse("2026-09-29T08:00:00Z");

interface FolderSpec {
  name: string;
  role: FolderRole;
  count: number;
  parent?: string;
}

const FOLDERS: FolderSpec[] = [
  { name: "Inbox", role: "inbox", count: 0 },
  { name: "Receipts", role: "other", count: 3, parent: "Inbox" },
  { name: "Sent", role: "sent", count: 4 },
  { name: "Drafts", role: "drafts", count: 0 },
  { name: "Archive", role: "archive", count: 6 },
  { name: "Junk", role: "junk", count: 1 },
  { name: "Trash", role: "trash", count: 0 },
];

function pick<T>(items: readonly T[], index: number): T {
  return items[index % items.length] as T;
}

/** Builds the mock mailbox. `inboxSize` controls the personal inbox (virtualization). */
export function createMockData(inboxSize = 250): MockData {
  const accounts: Account[] = [
    { id: "acc-personal", name: "Personal", email: "ada@example.org", kind: "imap" },
    { id: "acc-work", name: "Work", email: "ada@work.example", kind: "imap" },
  ];
  const folders: Folder[] = [];
  const messages: MessageSummary[] = [];
  const bodies = new Map<string, MessageBody>();

  for (const account of accounts) {
    const inboxCount = account.id === "acc-personal" ? inboxSize : 12;
    for (const spec of FOLDERS) {
      const id = `${account.id}/${spec.parent ? `${spec.parent}/` : ""}${spec.name}`;
      const parentId = spec.parent ? `${account.id}/${spec.parent}` : null;
      const count = spec.role === "inbox" ? inboxCount : spec.count;
      let unread = 0;
      for (let i = 0; i < count; i++) {
        const seq = messages.length;
        const from = pick(PEOPLE, seq);
        const subject = pick(SUBJECTS, seq + i);
        const seen = i % 3 !== 0;
        if (!seen) unread++;
        const messageId = `${id}#${i + 1}`;
        const date = new Date(MOCK_NOW - i * 37 * 60_000).toISOString();
        const text = `Hello Ada,\n\n${subject} (message ${i + 1} in ${spec.name}).\n\n> Earlier text\n>> Older text\n\nBest,\n${from.name ?? from.email}`;
        messages.push({
          id: messageId,
          folder_id: id,
          account_id: account.id,
          subject,
          from,
          to: [{ name: "Ada Lovelace", email: account.email }],
          date,
          flags: { seen, flagged: i % 7 === 0, answered: i % 11 === 0, draft: false },
          has_attachments: i % 5 === 0,
          preview: text.replace(/\s+/g, " ").slice(0, 120),
          thread_id: null,
          tags: [],
          duplicates: [],
        });
        bodies.set(messageId, {
          id: messageId,
          headers: {
            subject,
            from: [from],
            to: [{ name: "Ada Lovelace", email: account.email }],
            cc: i % 6 === 0 ? [pick(PEOPLE, seq + 1)] : [],
            reply_to: [],
            date,
            message_id: `${messageId.replaceAll("/", ".")}@mock.example`,
          },
          text,
          sanitized_html:
            i % 4 === 0
              ? `<p>Hello <b>Ada</b>,</p><p>${subject}.</p><blockquote><p>Earlier text</p></blockquote>`
              : null,
          attachments:
            i % 5 === 0
              ? [
                  {
                    index: 0,
                    filename: "report.pdf",
                    mime: "application/pdf",
                    size: 48_213,
                    inline: false,
                  },
                ]
              : [],
          remote_content_blocked: i % 8 === 0,
        });
      }
      folders.push({
        id,
        account_id: account.id,
        parent_id: parentId,
        name: spec.name,
        path: spec.parent ? `${spec.parent}/${spec.name}` : spec.name,
        role: spec.role,
        unread,
        total: count,
      });
    }
  }

  return {
    accounts,
    folders,
    messages,
    bodies,
    theme: defaultTheme(),
    layout: defaultLayout(),
    locale: null,
  };
}
