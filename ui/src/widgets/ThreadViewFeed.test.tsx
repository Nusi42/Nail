import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, cleanup, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { MOCK_CONVERSATION_SUBJECT } from "../api/mockConversations";
import { mockBody } from "../api/mockData";
import type { MessageSummary } from "../api/generated";
import { createEnv, renderNode, settleQueries, widget, type Env } from "../test/harness";
import { violations } from "../test/axe";
import { must } from "../test/must";
import { cardNodes } from "./ThreadViewFeed";
import {
  CARD_WIDGETS,
  feedItems,
  feedStore,
  initiallyOpen,
  primaryMessage,
  stepStop,
} from "./ThreadViewModel";
import { cardShowsSubject, scopedUiStore } from "./ThreadViewScope";
import { createUiStore } from "../state/uiStore";
import { selectMockConversation } from "../test/env";
import { INBOX } from "../test/mockIds";

afterEach(() => {
  cleanup();
  feedStore.getState().reset();
});

const msg = (id: string, seen = true) => ({ id, flags: { seen } });

describe("card feed rules (P2 WP203)", () => {
  it("opens the unread, the newest and the selected message", () => {
    const list = [msg("a"), msg("b", false), msg("c"), msg("d")];
    expect([...initiallyOpen(list, null)].sort()).toEqual(["b", "d"]);
    expect([...initiallyOpen(list, "a")].sort()).toEqual(["a", "b", "d"]);
    expect([...initiallyOpen(list, "zz")].sort()).toEqual(["b", "d"]);
    expect(initiallyOpen([], null).size).toBe(0);
  });

  it("scrolls to the list's older choice, else the newest unread, else the newest", () => {
    const list = [msg("a"), msg("b", false), msg("c"), msg("d")];
    expect(primaryMessage(list, "a")).toBe("a");
    expect(primaryMessage(list, "d")).toBe("b");
    expect(primaryMessage([msg("a"), msg("b")], "b")).toBe("b");
    expect(primaryMessage([], null)).toBeNull();
  });

  it("folds more than four collapsed cards in the middle, keeping the one before the next open card", () => {
    const list = ["m0", "m1", "m2", "m3", "m4", "m5", "m6", "m7"].map((id) => msg(id));
    const open = (m: { id: string }) => m.id === "m7";
    const items = feedItems(list, open, null, false);
    expect(
      items.map((i) => (i.kind === "fold" ? `fold:${String(i.messages.length)}` : i.message.id)),
    ).toEqual(["m0", "fold:5", "m6", "m7"]);
    // Four in a row stay; an open card in the middle splits the run.
    const split = feedItems(list, (m) => m.id === "m7" || m.id === "m3", null, false);
    expect(split.every((i) => i.kind === "card")).toBe(true);
    // The selected card is never folded away; unfolded shows everything.
    expect(feedItems(list, open, "m2", false).every((i) => i.kind === "card")).toBe(true);
    // The card shown beside the fold may be the kept one: nothing moves.
    expect(feedItems(list, open, "m6", false)).toEqual(feedItems(list, open, null, false));
    expect(feedItems(list, open, null, true)).toHaveLength(8);
    // Collapsing everything still keeps the first and the newest card.
    const closed = feedItems(list, () => false, null, false);
    expect(closed.map((i) => (i.kind === "fold" ? "fold" : i.message.id))).toEqual([
      "m0",
      "fold",
      "m6",
      "m7",
    ]);
  });

  it("steps through stops and stops at the ends", () => {
    expect(stepStop(["a", "b", "c"], "b", 1)).toBe("c");
    expect(stepStop(["a", "b", "c"], "c", 1)).toBeNull();
    expect(stepStop(["a", "b", "c"], "a", -1)).toBeNull();
    expect(stepStop(["a", "b", "c"], null, 1)).toBe("a");
    expect(stepStop(["a", "b", "c"], null, -1)).toBe("c");
    expect(stepStop([], null, 1)).toBeNull();
  });

  it("remembers what started open: reading a card never closes it", () => {
    const s = feedStore.getState();
    s.seed("t", ["a", "b"], { a: false, b: true });
    s.seed("t", ["a", "b", "c"], { a: true, b: false, c: true });
    expect(feedStore.getState().open.t).toEqual({ a: false, b: true, c: true });
    expect(feedStore.getState().order.t).toEqual(["a", "b", "c"]);
  });

  it("builds a card's widgets from card= and passes headers / mark-read to its message-view", () => {
    const all = cardNodes(widget("thread-view", { options: { mode: "cards" } }), false);
    expect(all.map((n) => n.kind)).toEqual([...CARD_WIDGETS]);
    expect(all.at(-1)?.options).toEqual({ headers: "", "mark-read": true });
    const some = cardNodes(
      widget("thread-view", {
        options: {
          card: "message-view remote-content plugin.x message-view",
          headers: "cc",
          "mark-read": false,
        },
      }),
      false,
    );
    expect(some.map((n) => n.kind)).toEqual(["message-view", "remote-content"]);
    expect(some[0]?.options).toEqual({ headers: "cc", "mark-read": false });
    // A message shown alone keeps message-view's own header fields.
    expect(cardNodes(widget("thread-view"), true).at(-1)?.options).toEqual({ "mark-read": true });
  });
});

describe("card widgets", () => {
  it("are the ones nail-config accepts for card=", () => {
    const rust = readFileSync(
      resolve(process.cwd(), "../crates/nail-config/src/widgets/core.rs"),
      "utf8",
    );
    const list = must(/pub\(crate\) const CARD_WIDGETS: &\[&str\] = &\[([^\]]*)\]/.exec(rust))[1];
    expect([...must(list).matchAll(/"([a-z-]+)"/g)].map((m) => m[1])).toEqual([...CARD_WIDGETS]);
  });
});

describe("scoped UI state", () => {
  it("shows a card's message as the selected one and stays stable between changes", () => {
    const parent = createUiStore({ selectedMessageId: "x" });
    const scoped = scopedUiStore(parent, "card");
    const first = scoped.getState();
    expect(first.selectedMessageId).toBe("card");
    expect(scoped.getState()).toBe(first);
    const seen: (string | null)[] = [];
    const stop = scoped.subscribe((state) => {
      seen.push(state.selectedMessageId);
    });
    parent.getState().selectFolder("acc-personal/Sent");
    expect(scoped.getState()).not.toBe(first);
    expect(scoped.getState().activeFolderId).toBe("acc-personal/Sent");
    expect(seen).toEqual(["card"]);
    // Writes go to the app's state.
    scoped.getState().selectMessage("y");
    expect(parent.getState().selectedMessageId).toBe("y");
    expect(scoped.getState().selectedMessageId).toBe("card");
    stop();
    // The card's own message selected: the very same state object.
    parent.getState().selectMessage("card");
    expect(scoped.getState()).toBe(parent.getState());
  });
});

/** Adds `count` older, read Inbox messages to the mock conversation. */
function addOlder(env: Env, first: MessageSummary, count: number): MessageSummary[] {
  const out: MessageSummary[] = [];
  for (let i = 0; i < count; i++) {
    const m: MessageSummary = {
      ...first,
      id: `${INBOX}#older-${String(i)}`,
      date: new Date(Date.parse(first.date) - (count - i) * 3_600_000).toISOString(),
      preview: `Older note ${String(i)}`,
      flags: { ...first.flags, seen: true },
    };
    env.transport.data.messages.push(m);
    env.transport.data.bodies.set(m.id, mockBody(m, { text: `Older note ${String(i)}` }));
    out.push(m);
  }
  return out;
}

function setup(older = 0) {
  const env = createEnv({ inboxSize: 12, state: { activeFolderId: INBOX } });
  const messages = selectMockConversation(env);
  const extra = older > 0 ? addOlder(env, must(messages[0]), older) : [];
  const newest = must(messages[5]);
  const view = renderNode(widget("thread-view", { options: { mode: "cards" } }), env);
  return { env, messages, extra, newest, ...view };
}

const cards = () =>
  screen.getAllByRole("article").filter((a) => a.dataset.part === "thread-view.card");

/** Waits until the fold opened (ten cards) with the first card it revealed focused. */
async function foldRevealed(toggleOf: (card: HTMLElement) => HTMLElement): Promise<void> {
  await waitFor(() => {
    expect(cards()).toHaveLength(10);
  });
  await waitFor(() => {
    expect(document.activeElement).toBe(toggleOf(must(cards()[1])));
  });
}

describe("thread-view mode=cards", () => {
  it("shows the conversation as cards, oldest first, the unread and newest open", async () => {
    const { env, messages, newest } = setup();
    const feed = await screen.findByRole("feed");
    const region = screen.getByRole("region", { name: "Conversation" });
    expect(
      within(region).getByRole("heading", { name: MOCK_CONVERSATION_SUBJECT }),
    ).toHaveAttribute("data-part", "thread-view.subject");
    expect(within(region).getByText("6 messages")).toBeInTheDocument();
    const all = cards();
    expect(all.map((c) => c.dataset.message)).toEqual(messages.slice(0, 6).map((m) => m.id));
    all.forEach((card, i) => {
      expect(card).toHaveAttribute("aria-setsize", "6");
      expect(card).toHaveAttribute("aria-posinset", String(i + 1));
      expect(feed.contains(card)).toBe(true);
    });
    const toggles = all.map((c) => must(c.querySelector('[data-part="thread-view.card.toggle"]')));
    expect(toggles.map((t) => t.getAttribute("aria-expanded"))).toEqual([
      "false",
      "false",
      "false",
      "false",
      "false",
      "true",
    ]);
    // The newest (unread, selected) card shows the reader's widgets for itself.
    const last = must(all[5]);
    expect(last.dataset.state?.split(" ")).toEqual(
      expect.arrayContaining(["expanded", "selected"]),
    );
    const view = await waitFor(() =>
      must(last.querySelector<HTMLElement>('[data-part="message-view"]')),
    );
    await within(view).findByText(/Minutes are in the shared folder/);
    expect(view).toHaveAttribute("data-message", newest.id);
    expect(within(view).queryByRole("heading", { level: 1 })).toBeNull();
    // Reading the open unread card marks it read (a queued write).
    await waitFor(() => {
      expect(env.transport.requests).toContainEqual(
        expect.objectContaining({ type: "UpdateFlags", message_ids: [newest.id] }),
      );
    });
    expect(last.querySelector('[data-part="thread-view.card.recipients"]')).toHaveTextContent(
      "to me, Alan Turing, Katherine Johnson",
    );
    // The user's own reply from Sent: "me", from-me, collapsed to one line.
    const mine = must(all[1]);
    expect(mine.dataset.state?.split(" ")).toContain("from-me");
    expect(within(mine).getByText("me")).toHaveAttribute("data-part", "thread-view.card.from");
    expect(mine.querySelector('[data-part="thread-view.card.preview"]')).toHaveTextContent(
      "Monday works for me.",
    );
    expect(mine.querySelector('[data-part="thread-view.card.body"]')).toBeNull();
    await settleQueries(env);
  });

  it("opens and closes cards, each with its own message", async () => {
    const user = userEvent.setup();
    const { env, messages } = setup();
    await screen.findByRole("feed");
    const first = must(cards()[0]);
    const toggle = within(first).getByRole("button", { expanded: false });
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "true");
    const view = await waitFor(() =>
      must(first.querySelector<HTMLElement>('[data-part="message-view"]')),
    );
    expect(view).toHaveAttribute("data-message", must(messages[0]).id);
    await within(view).findByText(/Agenda attached/);
    // Clicking into a card selects its message (the reader's actions act on it).
    expect(env.store.getState().selectedMessageId).toBe(must(messages[0]).id);
    expect(first).toHaveAttribute("aria-current", "true");
    await user.click(toggle);
    expect(toggle).toHaveAttribute("aria-expanded", "false");
    expect(first.querySelector('[data-part="thread-view.card.body"]')).toBeNull();
    // A reply's quoted lines fold away inside its card.
    const reply = must(cards()[1]);
    await user.click(within(reply).getByRole("button", { expanded: false }));
    const quote = await within(reply).findByText("Show quoted text");
    expect(quote).toHaveAttribute("data-part", "message-view.quote-toggle");
    expect(within(reply).getByText(/Monday works for me/)).toHaveAttribute(
      "data-part",
      "message-view.text-block",
    );
    // Expand all, collapse all.
    await user.click(screen.getByRole("button", { name: "Expand all" }));
    expect(cards().every((c) => c.dataset.state?.includes("expanded"))).toBe(true);
    await user.click(screen.getByRole("button", { name: "Collapse all" }));
    expect(cards().some((c) => c.dataset.state?.includes("expanded"))).toBe(false);
    await settleQueries(env);
  });

  it("folds the older read cards and opens the fold", async () => {
    const user = userEvent.setup();
    const { env, extra } = setup(4);
    await screen.findByRole("feed");
    // 10 messages: the 4 older ones and the first 5 of the conversation are
    // read; the first stays, the one before the newest stays as context.
    expect(cards()).toHaveLength(3);
    expect(cards().map((c) => c.getAttribute("aria-posinset"))).toEqual(["1", "9", "10"]);
    expect(must(cards()[0]).dataset.message).toBe(must(extra[0]).id);
    const fold = screen.getByRole("button", { name: "7 older messages" });
    expect(fold).toHaveAttribute("data-part", "thread-view.fold");
    expect(fold).toHaveAttribute("aria-expanded", "false");
    await user.click(fold);
    expect(cards()).toHaveLength(10);
    expect(screen.queryByRole("button", { name: /older message/ })).toBeNull();
    await settleQueries(env);
  });

  it("draws the fold as its own article between cards", async () => {
    const { env } = setup(4);
    const feed = await screen.findByRole("feed");
    const fold = screen.getByRole("button", { name: "7 older messages" });
    const row = must(fold.closest("article"));
    expect(row).toHaveAttribute("data-part", "thread-view.fold-row");
    expect(row.parentElement).toBe(feed);
    expect(screen.getByRole("article", { name: "7 older messages" })).toBe(row);
    // Every card stays a card: no fold inside one.
    expect(cards().some((c) => c.querySelector('[data-part="thread-view.fold"]') !== null)).toBe(
      false,
    );
    await settleQueries(env);
  });

  it("keeps the focus in the feed when the fold opens from the keyboard", async () => {
    const user = userEvent.setup();
    const { env, extra } = setup(4);
    await screen.findByRole("feed");
    const fold = screen.getByRole("button", { name: "7 older messages" });
    act(() => {
      fold.focus();
    });
    await user.keyboard("{Enter}");
    const toggleOf = (card: HTMLElement) =>
      must(card.querySelector<HTMLElement>('[data-part="thread-view.card.toggle"]'));
    // The first card the fold revealed holds the focus, so j goes on.
    await foldRevealed(toggleOf);
    expect(env.store.getState().selectedMessageId).toBe(must(extra[1]).id);
    await user.keyboard("j");
    expect(document.activeElement).toBe(toggleOf(must(cards()[2])));
    expect(env.store.getState().selectedMessageId).toBe(must(extra[2]).id);
    await settleQueries(env);
  });

  it("moves between cards with j / k, through folds with n / p, toggles with o and answers with r", async () => {
    const user = userEvent.setup();
    const { env, messages, extra } = setup(4);
    await screen.findByRole("feed");
    const toggleOf = (card: HTMLElement) =>
      must(card.querySelector<HTMLElement>('[data-part="thread-view.card.toggle"]'));
    act(() => {
      toggleOf(must(cards()[0])).focus();
    });
    expect(env.store.getState().selectedMessageId).toBe(must(extra[0]).id);
    // j: the fold is the next stop, then the card after it.
    await user.keyboard("j");
    expect(document.activeElement).toHaveAttribute("data-part", "thread-view.fold");
    await user.keyboard("j");
    expect(document.activeElement).toBe(toggleOf(must(cards()[1])));
    expect(env.store.getState().selectedMessageId).toBe(must(messages[4]).id);
    await user.keyboard("k");
    await user.keyboard("k");
    expect(document.activeElement).toBe(toggleOf(must(cards()[0])));
    // n: the next message is behind the fold, which opens.
    await user.keyboard("n");
    await foldRevealed(toggleOf);
    expect(env.store.getState().selectedMessageId).toBe(must(extra[1]).id);
    await user.keyboard("p");
    expect(document.activeElement).toBe(toggleOf(must(cards()[0])));
    // o opens the focused card, : closes all, ; opens all.
    await user.keyboard("o");
    expect(toggleOf(must(cards()[0]))).toHaveAttribute("aria-expanded", "true");
    await user.keyboard(":");
    expect(cards().some((c) => c.dataset.state?.includes("expanded"))).toBe(false);
    await user.keyboard(";");
    expect(cards().every((c) => c.dataset.state?.includes("expanded"))).toBe(true);
    // r answers the focused card.
    await user.keyboard("r");
    await waitFor(() => {
      expect(env.transport.requests).toContainEqual(
        expect.objectContaining({
          type: "NewDraft",
          context: { mode: "reply", message_id: must(extra[0]).id },
        }),
      );
    });
    await settleQueries(env);
  });

  it("answers a card from its own buttons", async () => {
    const user = userEvent.setup();
    const { env, messages } = setup();
    await screen.findByRole("feed");
    const first = must(cards()[0]);
    await user.click(within(first).getByRole("button", { expanded: false }));
    const actions = within(first).getByRole("group", {
      name: "Actions for the message from Grace Hopper",
    });
    await user.click(within(actions).getByRole("button", { name: "Forward" }));
    await waitFor(() => {
      expect(env.transport.requests).toContainEqual(
        expect.objectContaining({
          type: "NewDraft",
          context: { mode: "forward", message_id: must(messages[0]).id },
        }),
      );
    });
    await settleQueries(env);
  });

  it("shows the selected message alone without a conversation", async () => {
    const env = createEnv({ inboxSize: 12, state: { activeFolderId: INBOX } });
    const { container } = renderNode(widget("thread-view", { options: { mode: "cards" } }), env);
    expect(container.querySelector('[data-part="thread-view"]')).toHaveAttribute(
      "data-mode",
      "single",
    );
    expect(screen.queryByRole("feed")).toBeNull();
    // message-view's own empty state.
    expect(container.querySelector('[data-part="message-view.empty"]')).not.toBeNull();
    act(() => {
      env.store.getState().selectMessage(`${INBOX}#1`);
    });
    const view = await waitFor(() =>
      must(container.querySelector<HTMLElement>('[data-part="message-view"]')),
    );
    await within(view).findByRole("heading", { level: 1 });
    await settleQueries(env);
  });

  it("has no axe violations", async () => {
    const user = userEvent.setup();
    const { env, container } = setup(4);
    await screen.findByRole("feed");
    await settleQueries(env);
    expect(await violations(container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Expand all" }));
    await settleQueries(env);
    expect(await violations(container)).toEqual([]);
  });
});

describe("a card's own subject (P2 integration)", () => {
  const card = { messageId: "m", subject: "Lab move: timeline" };

  it("stays hidden when it repeats the conversation's subject", () => {
    expect(cardShowsSubject("Re: Lab move: timeline", card)).toBe(false);
    expect(cardShowsSubject("FWD: re: lab move: TIMELINE", card)).toBe(false);
    expect(cardShowsSubject("", card)).toBe(false);
    // A mailing list's tag is no new subject either.
    expect(cardShowsSubject("Re: [lab] Lab move: timeline", card)).toBe(false);
  });

  it("shows when it says something else (an encrypted message's real subject)", () => {
    expect(cardShowsSubject("Secret plans", { messageId: "m", subject: "..." })).toBe(true);
    expect(cardShowsSubject("Re: Lab move: new date", card)).toBe(true);
  });
});
