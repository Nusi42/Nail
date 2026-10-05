/**
 * P2 WP202 (ADR 0148): conversation rows (Design A) and the opt-in rows that
 * open into their messages (Design B, `threads="expandable"`), on the mock
 * core with the Nightingale conversation (four messages in the Inbox, two of
 * the user's replies from Sent; Ada is the user).
 */
import { act, cleanup, fireEvent, screen, waitFor, within } from "@testing-library/react";
import type { KeyBinding } from "../api/generated";
import {
  MOCK_CONVERSATION_ID,
  MOCK_CONVERSATION_SUBJECT,
  addMockConversation,
} from "../api/mockConversations";
import {
  createEnv,
  renderNode,
  settleQueries,
  stubElementSizes,
  widget,
  type Env,
} from "../test/harness";
import { violations } from "../test/axe";
import { must } from "../test/must";
import { currentListControl } from "./messageListControl";
import { listStep, templateFields } from "./MessageList";
import { compileTemplate } from "../template/template";

const INBOX = "acc-personal/Inbox";
const SENT = "acc-personal/Sent";
/** The default layout's wide rows (Design A). */
const ROWS = [
  "{participants | names(3)}  {count | if-many}  {date | relative}",
  "{subject}",
  '{latest_from | names(1) | then(":")}{preview}',
];

let restoreSizes: () => void;
beforeEach(() => {
  restoreSizes = stubElementSizes();
});
afterEach(() => {
  restoreSizes();
});

function setup(threads: "collapsed" | "expandable"): Env {
  const env = createEnv({
    inboxSize: 6,
    state: { activeAccountId: "acc-personal", activeFolderId: INBOX },
  });
  addMockConversation(env.transport.data);
  renderNode(widget("message-list", { options: { threads, "page-size": 50 }, rows: ROWS }), env);
  return env;
}

async function nightingale(): Promise<HTMLElement> {
  return waitFor(() =>
    must(
      document.querySelector<HTMLElement>(
        `[data-part="message-list.row"][data-thread="${MOCK_CONVERSATION_ID}"]`,
      ),
    ),
  );
}

/** A row of one message (its count pill is empty). */
const isSingle = (row: Element) =>
  row.querySelector('[data-part="message-list.row.count"]')?.textContent === "";

const part = (root: Element, name: string) =>
  must(root.querySelector<HTMLElement>(`[data-part="${name}"]`));

describe("conversation rows (Design A)", () => {
  it("shows up to three people with me, unread senders bold, the count and Sender: preview", async () => {
    const env = setup("collapsed");
    const row = await nightingale();
    const people = part(row, "message-list.row.participants");
    const names = within(people)
      .getAllByText(/./, { selector: '[data-part="message-list.row.participant"]' })
      .map((n) => [n.textContent, n.getAttribute("data-state")]);
    // Grace, me (Ada), Alan, Katherine: the first two and the newest sender.
    expect(names).toEqual([
      ["Grace Hopper", "unread"],
      ["me", "me"],
      ["Katherine Johnson", null],
    ]);
    expect(people).toHaveTextContent("Grace Hopper, me … Katherine Johnson");
    expect(part(row, "message-list.row.count")).toHaveTextContent("6");
    expect(part(row, "message-list.row.subject")).toHaveTextContent(MOCK_CONVERSATION_SUBJECT);
    const latest = part(row, "message-list.row.latest-from");
    expect(latest).toHaveTextContent("Grace Hopper:");
    expect(part(row, "message-list.row.preview")).toHaveTextContent(/^Minutes are in the shared/);
    // The preview line is marked for Compact (which hides it).
    expect(latest.closest('[data-part="message-list.row.line"]')).toHaveAttribute(
      "data-fields",
      "latest-from preview",
    );
    expect(row).toHaveAttribute("role", "option");
    await settleQueries(env);
  });

  it("hides the count and the chevron of a single message", async () => {
    const env = setup("collapsed");
    await nightingale();
    const single = must(screen.getAllByRole("option").find(isSingle));
    expect(part(single, "message-list.row.count")).toBeEmptyDOMElement();
    expect(single.querySelector('[data-part="message-list.row.expander"]')).toBeNull();
    await settleQueries(env);
  });

  it("selects the conversation's newest message and its thread", async () => {
    const env = setup("collapsed");
    fireEvent.click(await nightingale());
    expect(env.store.getState().selectedMessageId).toBe(`${INBOX}#nightingale-6`);
    expect(env.store.getState().selectedThreadId).toBe(MOCK_CONVERSATION_ID);
    await settleQueries(env);
  });
});

describe("rows that open into their messages (Design B)", () => {
  async function expand(env: Env) {
    const row = await nightingale();
    fireEvent.click(part(row, "message-list.row.expander"));
    await waitFor(() => {
      expect(document.querySelectorAll('[data-part="message-list.child"]')).toHaveLength(6);
    });
    await settleQueries(env);
    return row;
  }

  it("is a tree whose conversation rows open with the chevron", async () => {
    const env = setup("expandable");
    const tree = await screen.findByRole("tree", { name: "Messages" });
    const row = await nightingale();
    expect(row).toHaveAttribute("role", "treeitem");
    expect(row).toHaveAttribute("aria-level", "1");
    expect(row).toHaveAttribute("aria-expanded", "false");
    await expand(env);
    expect(row).toHaveAttribute("aria-expanded", "true");
    expect(row).toHaveAttribute("data-state", expect.stringMatching(/\bexpanded\b/));
    // The chevron does not choose the row.
    expect(env.store.getState().selectedMessageId).toBeNull();

    const children = [...tree.querySelectorAll<HTMLElement>('[data-part="message-list.child"]')];
    // Oldest first, the user's replies from Sent among them as "me".
    expect(children.map((c) => part(c, "message-list.child.from").textContent)).toEqual([
      "Grace Hopper",
      "me",
      "Alan Turing",
      "Katherine Johnson",
      "me",
      "Grace Hopper",
    ]);
    const first = must(children[0]);
    expect(first).toHaveAttribute("aria-level", "2");
    expect(first).toHaveAttribute("aria-posinset", "1");
    expect(first).toHaveAttribute("aria-setsize", "6");
    expect(children[1]).toHaveAttribute("data-state", "from-me");
    expect(children[1]).toHaveAttribute("data-folder", SENT);
    expect(children[5]).toHaveAttribute("data-state", "unread");
    expect(part(first, "message-list.child.preview")).toHaveTextContent(/^Hi all,/);
    expect(part(first, "message-list.child.date").textContent).not.toBe("");
    // Rows without more than one message have no chevron (but keep its place).
    const single = must(
      [...tree.querySelectorAll('[data-part="message-list.row"]')].find(isSingle),
    );
    expect(single).not.toHaveAttribute("aria-expanded");
    expect(part(single, "message-list.row.expander")).toHaveAttribute("data-state", "single");

    expect(await violations(tree)).toEqual([]);
  });

  it("opens a chosen child alone in the reader, then the conversation again", async () => {
    const env = setup("expandable");
    const row = await expand(env);
    const children = document.querySelectorAll<HTMLElement>('[data-part="message-list.child"]');
    fireEvent.click(must(children[5]));
    // The newest child selects the same message as its conversation row: the
    // child, not the row, is selected, and the reader shows it alone.
    expect(env.store.getState().selectedMessageId).toBe(`${INBOX}#nightingale-6`);
    expect(env.store.getState().selectedThreadId).toBeNull();
    expect(children[5]).toHaveAttribute("aria-selected", "true");
    expect(row).toHaveAttribute("aria-selected", "false");
    fireEvent.click(row);
    expect(env.store.getState().selectedThreadId).toBe(MOCK_CONVERSATION_ID);
    expect(row).toHaveAttribute("aria-selected", "true");
    expect(children[5]).toHaveAttribute("aria-selected", "false");
    // Each message once in the order the reader's j / k use.
    const order = env.store.getState().messageOrder;
    expect(new Set(order).size).toBe(order.length);
    await settleQueries(env);
  });

  it("steps through conversations and children with j / k and opens / closes with → / ←", async () => {
    const env = setup("expandable");
    const tree = await screen.findByRole("tree");
    const row = await nightingale();
    fireEvent.click(row);
    act(() => {
      tree.focus();
    });
    expect(currentListControl()).not.toBeNull();
    const selectedKey = () =>
      must(tree.querySelector('[aria-selected="true"]')).getAttribute("data-message");
    const selectedPart = () =>
      must(tree.querySelector('[aria-selected="true"]')).getAttribute("data-part");

    fireEvent.keyDown(tree, { key: "ArrowRight" });
    await waitFor(() => {
      expect(row).toHaveAttribute("aria-expanded", "true");
    });
    await waitFor(() => {
      expect(tree.querySelectorAll('[data-part="message-list.child"]')).toHaveLength(6);
    });
    // → again moves into the conversation.
    fireEvent.keyDown(tree, { key: "ArrowRight" });
    expect(selectedPart()).toBe("message-list.child");
    expect(selectedKey()).toBe(`${INBOX}#nightingale-1`);
    fireEvent.keyDown(tree, { key: "j" });
    expect(selectedKey()).toBe(`${SENT}#nightingale-2`);
    expect(env.store.getState().selectedMessageId).toBe(`${SENT}#nightingale-2`);
    for (let i = 0; i < 4; i++) fireEvent.keyDown(tree, { key: "ArrowDown" });
    // The newest child: the same message as the conversation row, but the child.
    expect(selectedPart()).toBe("message-list.child");
    expect(selectedKey()).toBe(`${INBOX}#nightingale-6`);
    fireEvent.keyDown(tree, { key: "k" });
    expect(selectedKey()).toBe(`${SENT}#nightingale-5`);
    // ← goes to the conversation, ← again closes it.
    fireEvent.keyDown(tree, { key: "ArrowLeft" });
    expect(selectedPart()).toBe("message-list.row");
    expect(env.store.getState().selectedThreadId).toBe(MOCK_CONVERSATION_ID);
    fireEvent.keyDown(tree, { key: "ArrowLeft" });
    await waitFor(() => {
      expect(row).toHaveAttribute("aria-expanded", "false");
    });
    expect(tree.querySelectorAll('[data-part="message-list.child"]')).toHaveLength(0);
    // j from the closed conversation goes to the next conversation.
    fireEvent.keyDown(tree, { key: "j" });
    expect(selectedPart()).toBe("message-list.row");
    expect(selectedKey()).not.toBe(`${INBOX}#nightingale-6`);
    await settleQueries(env);
  });

  it("keeps the selection on the conversation when it closes under a child", async () => {
    const env = setup("expandable");
    const row = await expand(env);
    fireEvent.click(
      must(document.querySelectorAll<HTMLElement>('[data-part="message-list.child"]')[2]),
    );
    fireEvent.click(part(row, "message-list.row.expander"));
    await waitFor(() => {
      expect(row).toHaveAttribute("aria-selected", "true");
    });
    expect(env.store.getState().selectedMessageId).toBe(`${INBOX}#nightingale-6`);
    expect(env.store.getState().selectedThreadId).toBe(MOCK_CONVERSATION_ID);
    await settleQueries(env);
  });

  it("keeps open conversations when the list mounts again (a breakpoint switch)", async () => {
    const env = setup("expandable");
    await expand(env);
    cleanup();
    renderNode(
      widget("message-list", { options: { threads: "expandable", "page-size": 50 }, rows: ROWS }),
      env,
    );
    const row = await nightingale();
    expect(row).toHaveAttribute("aria-expanded", "true");
    await waitFor(() => {
      expect(document.querySelectorAll('[data-part="message-list.child"]')).toHaveLength(6);
    });
    await settleQueries(env);
  });

  it("has no list control while the list does not expand", async () => {
    const env = setup("collapsed");
    await nightingale();
    expect(currentListControl()).toBeNull();
    await settleQueries(env);
  });
});

describe("helpers", () => {
  it("finds the list's step for a typed key from the layout's bindings", () => {
    const keys: KeyBinding[] = [
      { context: "reader", key: "n", action: "mail.next", args: [] },
      { context: "list", key: "J", action: "mail.next", args: [] },
      { context: "list", key: "k", action: "mail.previous", args: [] },
      { context: "list", key: "g j", action: "mail.next", args: [] },
      { context: "list", key: "x", action: "mail.archive", args: [] },
    ];
    expect(listStep(keys, "j")).toBe(1);
    expect(listStep(keys, "k")).toBe(-1);
    expect(listStep(keys, "n")).toBeNull();
    expect(listStep(keys, "x")).toBeNull();
    expect(listStep(keys, "g")).toBeNull();
  });

  it("names the fields of a row template", () => {
    expect(templateFields(compileTemplate(must(ROWS[2])))).toBe("latest-from preview");
    expect(templateFields(compileTemplate("{date} {date | relative}"))).toBe("date");
    expect(templateFields(compileTemplate("plain"))).toBeUndefined();
  });
});
