/**
 * A conversation for the P2 packages (ADR 0148) to build against in the
 * mock core: six messages in the personal account's Inbox and Sent (two of
 * them the user's own replies, one unread, one with three more people)
 * plus a draft reply that never joins it. The generated mailbox's threads
 * stay within one folder, so tests that need a conversation across folders
 * add this one: `addMockConversation(transport.data)`.
 */
import type { Address, MessageSummary } from "./generated";
import { MOCK_NOW, mockBody, mockFolderId, type MockData } from "./mockData";

/** The thread id of the added conversation. */
export const MOCK_CONVERSATION_ID = "t-nightingale";
/** Its subject (the first message's; replies add "Re: "). */
export const MOCK_CONVERSATION_SUBJECT = "Project Nightingale kickoff";

const ADA: Address = { name: "Ada Lovelace", email: "ada@example.org" };
const GRACE: Address = { name: "Grace Hopper", email: "grace@navy.example" };
const ALAN: Address = { name: "Alan Turing", email: "alan@bletchley.example" };
const KATHERINE: Address = { name: "Katherine Johnson", email: "katherine@nasa.example" };

interface Spec {
  folder: "Inbox" | "Sent" | "Drafts";
  from: Address;
  minutesAgo: number;
  seen: boolean;
  text: string;
}

const SPECS: readonly Spec[] = [
  {
    folder: "Inbox",
    from: GRACE,
    minutesAgo: 3 * 24 * 60,
    seen: true,
    text: "Hi all,\n\nLet's start Project Nightingale on Monday. Agenda attached.\n\nGrace",
  },
  {
    folder: "Sent",
    from: ADA,
    minutesAgo: 3 * 24 * 60 - 45,
    seen: true,
    text: "Monday works for me. I'll bring the numbers.\n\nAda\n\n> Let's start Project Nightingale on Monday.",
  },
  {
    folder: "Inbox",
    from: ALAN,
    minutesAgo: 2 * 24 * 60,
    seen: true,
    text: "Count me in. Can we move it to 10:00?\n\nAlan\n\n> Monday works for me.",
  },
  {
    folder: "Inbox",
    from: KATHERINE,
    minutesAgo: 24 * 60,
    seen: true,
    text: "10:00 is fine. I booked room 3.\n\nKatherine",
  },
  {
    folder: "Sent",
    from: ADA,
    minutesAgo: 20 * 60,
    seen: true,
    text: "Thanks, Katherine. See you all there.\n\nAda",
  },
  {
    folder: "Inbox",
    from: GRACE,
    minutesAgo: 90,
    seen: false,
    text: "Minutes are in the shared folder. Next meeting in two weeks.\n\nGrace",
  },
  {
    folder: "Drafts",
    from: ADA,
    minutesAgo: 30,
    seen: true,
    text: "Draft: thanks for the minutes",
  },
];

/**
 * Adds the conversation to `data` (personal account) and returns its
 * messages, oldest first, including the draft (last).
 */
export function addMockConversation(data: MockData): MessageSummary[] {
  const account = "acc-personal";
  const added: MessageSummary[] = [];
  SPECS.forEach((spec, index) => {
    const folderId = mockFolderId(account, spec.folder);
    const summary: MessageSummary = {
      id: `${folderId}#nightingale-${String(index + 1)}`,
      folder_id: folderId,
      account_id: account,
      subject: index === 0 ? MOCK_CONVERSATION_SUBJECT : `Re: ${MOCK_CONVERSATION_SUBJECT}`,
      from: spec.from,
      to: [ADA, GRACE, ALAN, KATHERINE].filter((a) => a.email !== spec.from.email),
      date: new Date(MOCK_NOW - spec.minutesAgo * 60_000).toISOString(),
      flags: { seen: spec.seen, flagged: false, answered: false, draft: spec.folder === "Drafts" },
      has_attachments: index === 0,
      preview: spec.text.replace(/\s+/g, " ").slice(0, 120),
      thread_id: MOCK_CONVERSATION_ID,
      tags: [],
      duplicates: [],
    };
    data.messages.push(summary);
    data.bodies.set(summary.id, mockBody(summary, { text: spec.text }));
    const folder = data.folders.find((f) => f.id === folderId);
    if (folder) {
      folder.total++;
      if (!spec.seen) folder.unread++;
    }
    added.push(summary);
  });
  return added;
}
