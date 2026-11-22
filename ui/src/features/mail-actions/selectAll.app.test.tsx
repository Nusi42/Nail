/**
 * "Select all" in the message list through the whole app (R5, ADR 0235)
 * against the mock core: Ctrl+A selects every row of the list, loaded or
 * not; Ctrl + click leaves a row out; the list says "All N selected" with
 * the core's count; an action asks once with the true count and sends one
 * list target; Esc and another folder end it.
 */
import { act, fireEvent, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { KeyBinding, Request } from "../../api/generated";
import { violations } from "../../test/axe";
import { appWindowEachTest, eventually, renderApp, settleUntil } from "../../test/app";
import { createEnv, type Env } from "../../test/harness";
import { must } from "../../test/must";
import { INBOX, SENT } from "../../test/mockIds";
import { listSelectionFor } from "../../actions/mailTargets";
import { BULK_CONFIRM_DIALOG } from "./bulk";
import { CLEAR_SELECTION_ACTION, SELECT_ALL_ACTION, TOGGLE_ROW_ACTION } from "./index";

appWindowEachTest();

/** More conversations than the list loads in its first page (100 rows). */
const INBOX_SIZE = 400;

/**
 * The list's select-all keys (ADR 0235): Ctrl+A, Cmd+A on macOS and Esc,
 * as the default layout (`defaults/layout.kdl`) binds them.
 */
const SELECT_KEYS: KeyBinding[] = [
  { context: "list", key: "ctrl+a", action: SELECT_ALL_ACTION, args: [] },
  { context: "list", key: "meta+a", action: SELECT_ALL_ACTION, args: [] },
  { context: "list", key: "escape", action: CLEAR_SELECTION_ACTION, args: [] },
  { context: "list", key: "ctrl+space", action: TOGGLE_ROW_ACTION, args: [] },
];

function renderInbox(): Env {
  const env = createEnv({
    inboxSize: INBOX_SIZE,
    state: { connection: "connecting", activeAccountId: "acc-personal", activeFolderId: INBOX },
  });
  // The mock serves the default layout (`default-layout.json`).
  expect(env.transport.data.layout.keys).toEqual(expect.arrayContaining(SELECT_KEYS));
  renderApp(env);
  return env;
}

const list = () => must(document.querySelector<HTMLElement>('[data-part="message-list"]'));
const rows = () => [...document.querySelectorAll<HTMLElement>('[data-part="message-list.row"]')];
const summary = () =>
  document.querySelector<HTMLElement>('[data-part="message-list.selection-summary"]');
/** The polite live region beside the list box (screen readers). */
const announcer = () =>
  must(document.querySelector<HTMLElement>('[data-part="message-list.announcer"]'));
const selectedCount = () => rows().filter((r) => r.getAttribute("aria-selected") === "true").length;
const applies = (env: Env) =>
  env.transport.requests.filter(
    (r): r is Extract<Request, { type: "ApplyMailAction" }> => r.type === "ApplyMailAction",
  );
const inboxMessages = (env: Env) =>
  env.transport.data.messages.filter((m) => m.folder_id === INBOX);

/** The Inbox's conversations (the list's true row count). */
const conversationCount = (env: Env) => new Set(inboxMessages(env).map((m) => m.thread_id)).size;

/** Opens the Inbox with its first rows and gives the list the focus. */
async function focusedList(env: Env): Promise<HTMLElement> {
  await settleUntil(() => rows()[0] ?? null, "the Inbox's first row");
  fireEvent.click(must(rows()[0]));
  expect(env.store.getState().activeFolderId).toBe(INBOX);
  // Fewer rows loaded than the list holds.
  expect(rows().length).toBeLessThan(INBOX_SIZE);
  return list();
}

/** Ctrl+A in the list, until it says "All N selected". */
async function pressSelectAll(listBox: HTMLElement): Promise<void> {
  fireEvent.keyDown(listBox, { key: "a", ctrlKey: true });
  await eventually(() => {
    expect(summary()).not.toBeNull();
  });
}

/**
 * Until the opened row's read mark (a queued write) has replayed and the
 * status bar has its summary: a test that ends earlier gets that update
 * after its end, outside `act`.
 */
async function readMarkSettled(env: Env): Promise<void> {
  await eventually(() => {
    expect(env.transport.requests.some((r) => r.type === "UpdateFlags")).toBe(true);
    expect(env.transport.data.opQueue.size).toBe(0);
    expect(env.store.getState().opsSummary).toMatchObject({ pending: 0, in_flight: 0 });
  });
}

/** The open question before an action on the whole list. */
const bulkQuestion = () =>
  settleUntil(
    () => document.querySelector<HTMLElement>('[data-part="mail-confirm"][data-confirm="bulk"]'),
    "the question",
  );

describe("select all in the message list (R5)", () => {
  it("Ctrl+A selects the whole list, Ctrl + click leaves a row out, the action asks once", async () => {
    const user = userEvent.setup();
    const env = renderInbox();
    const listBox = await focusedList(env);
    expect(listBox).toHaveAttribute("aria-multiselectable", "true");
    const total = conversationCount(env);
    expect(total).toBeGreaterThan(100);

    // The live region is there (empty) before Ctrl+A, outside the list
    // box, so the change is announced and the list box holds options only.
    expect(announcer()).toHaveTextContent("");
    expect(announcer()).toHaveAttribute("aria-live", "polite");
    expect(listBox.contains(announcer())).toBe(false);
    fireEvent.keyDown(listBox, { key: "a", ctrlKey: true });
    await eventually(() => {
      expect(summary()).toHaveTextContent(`All ${String(total)} selected`);
    });
    expect(announcer()).toHaveTextContent(`All ${String(total)} selected`);
    expect(summary()).not.toHaveAttribute("aria-live");
    expect(await violations(must(listBox.parentElement))).toEqual([]);
    expect(rows().every((r) => r.getAttribute("aria-selected") === "true")).toBe(true);
    // The count comes from the core.
    expect(env.transport.requests.some((r) => r.type === "ResolveMailTargets")).toBe(true);

    // Ctrl + click leaves a row out (and takes it back).
    const second = must(rows()[1]);
    fireEvent.click(second, { ctrlKey: true });
    expect(second).toHaveAttribute("aria-selected", "false");
    await eventually(() => {
      expect(summary()).toHaveTextContent(`All ${String(total - 1)} selected`);
    });
    const leftThread = must(second.dataset.thread);
    const leftMessages = inboxMessages(env).filter((m) => m.thread_id === leftThread).length;

    // Archive (e): one question with the true count, then one request.
    fireEvent.keyDown(listBox, { key: "e" });
    const dialog = await bulkQuestion();
    expect(env.store.getState().windows).toContain(BULK_CONFIRM_DIALOG);
    expect(dialog).toHaveTextContent(
      `This affects all ${String(total - 1)} selected conversations (${String(INBOX_SIZE - leftMessages)} messages).`,
    );
    expect(applies(env)).toEqual([]);
    expect(await violations(dialog)).toEqual([]);
    await user.click(within(dialog).getByRole("button", { name: "Archive all" }));
    await eventually(() => {
      expect(applies(env)).toHaveLength(1);
    });
    const [target] = must(applies(env)[0]).targets;
    expect(target).toMatchObject({ kind: "list", except: [expect.anything()] });
    expect(target?.kind === "list" && target.as_of).toBeTruthy();
    // Everything left the Inbox but the row left out.
    await eventually(() => {
      expect(inboxMessages(env)).toHaveLength(leftMessages);
    });
    expect(inboxMessages(env).every((m) => m.thread_id === leftThread)).toBe(true);
    // One batch, one undo toast; "select all" ended with the rows.
    await eventually(() => {
      expect(screen.getByRole("button", { name: "Undo" })).toBeInTheDocument();
    });
    expect(summary()).toBeNull();
  });

  it("Esc ends select all (rows loaded later: e2e r5-select-all)", async () => {
    const env = renderInbox();
    const listBox = await focusedList(env);
    await pressSelectAll(listBox);
    fireEvent.keyDown(listBox, { key: "Escape" });
    await eventually(() => {
      expect(summary()).toBeNull();
    });
    expect(announcer()).toHaveTextContent("Selection cleared");
    expect(listSelectionFor(env.store).getState().all).toBeNull();
    expect(rows().filter((r) => r.getAttribute("aria-selected") === "true").length).toBeLessThan(2);
  });

  it("another folder ends select all, and a cancelled question acts on nothing", async () => {
    const user = userEvent.setup();
    const env = renderInbox();
    const listBox = await focusedList(env);
    await pressSelectAll(listBox);
    fireEvent.keyDown(listBox, { key: "e" });
    const dialog = await bulkQuestion();
    await user.click(within(dialog).getByRole("button", { name: "Cancel" }));
    await eventually(() => {
      expect(env.store.getState().windows).not.toContain(BULK_CONFIRM_DIALOG);
    });
    expect(applies(env)).toEqual([]);
    expect(inboxMessages(env)).toHaveLength(INBOX_SIZE);
    expect(summary()).not.toBeNull();

    act(() => {
      env.store.getState().selectFolder("acc-personal/Sent");
    });
    await eventually(() => {
      expect(summary()).toBeNull();
    });
    expect(listSelectionFor(env.store).getState().all).toBeNull();
  });

  it("Ctrl+Space leaves the selected row out and takes it back; Shift+Arrow keeps the whole list", async () => {
    const env = renderInbox();
    const listBox = await focusedList(env);
    const total = conversationCount(env);
    await pressSelectAll(listBox);
    // The first row is the selected one: Ctrl+Space leaves it out.
    const first = must(rows()[0]);
    fireEvent.keyDown(listBox, { key: " ", code: "Space", ctrlKey: true });
    await eventually(() => {
      expect(first).toHaveAttribute("aria-selected", "false");
    });
    await eventually(() => {
      expect(summary()).toHaveTextContent(`All ${String(total - 1)} selected`);
    });
    fireEvent.keyDown(listBox, { key: " ", code: "Space", ctrlKey: true });
    await eventually(() => {
      expect(first).toHaveAttribute("aria-selected", "true");
    });
    // Shift+Arrow under "select all" works like Shift + click: the rows
    // are left out, the whole list stays selected.
    fireEvent.keyDown(listBox, { key: "ArrowDown", shiftKey: true });
    await eventually(() => {
      expect(must(rows()[1])).toHaveAttribute("aria-selected", "false");
    });
    expect(listSelectionFor(env.store).getState().all).not.toBeNull();
    expect(summary()).not.toBeNull();
    expect(selectedCount()).toBe(rows().length - 2);
    await eventually(() => {
      expect(summary()).toHaveTextContent(`All ${String(total - 2)} selected`);
    });
  });

  it("the row's menu selects the whole list", async () => {
    const env = renderInbox();
    await focusedList(env);
    const total = conversationCount(env);
    fireEvent.contextMenu(must(rows()[2]), { clientX: 40, clientY: 50, button: 2 });
    const menu = await screen.findByRole("menu", { name: "Message actions" });
    fireEvent.click(within(menu).getByRole("menuitem", { name: /Select all/ }));
    await eventually(() => {
      expect(summary()).toHaveTextContent(`All ${String(total)} selected`);
    });
    expect(selectedCount()).toBe(rows().length);
    await readMarkSettled(env);
  });

  it("another list never shows the previous list's count while its own loads", async () => {
    const env = renderInbox();
    const listBox = await focusedList(env);
    const inboxTotal = conversationCount(env);
    await pressSelectAll(listBox);
    await eventually(() => {
      expect(summary()).toHaveTextContent(`All ${String(inboxTotal)} selected`);
    });
    fireEvent.keyDown(listBox, { key: "Escape" });
    await eventually(() => {
      expect(summary()).toBeNull();
    });

    // Another list (the Sent folder); its count waits for the test.
    const sentTotal = new Set(
      env.transport.data.messages.filter((m) => m.folder_id === SENT).map((m) => m.thread_id),
    ).size;
    expect(sentTotal).toBeGreaterThan(0);
    expect(sentTotal).not.toBe(inboxTotal);
    act(() => {
      env.store.getState().selectFolder(SENT);
    });
    await settleUntil(
      () => rows().find((r) => r.getAttribute("aria-selected") !== "true") ?? null,
      "the Sent folder's rows",
    );
    const gate: { release?: () => void } = {};
    const held = new Promise<void>((resolve) => {
      gate.release = resolve;
    });
    const send = env.transport.send.bind(env.transport);
    env.transport.send = async (request) => {
      if (request.type === "ResolveMailTargets") await held;
      return send(request);
    };
    fireEvent.keyDown(list(), { key: "a", ctrlKey: true });
    await eventually(() => {
      expect(summary()).toHaveTextContent(/^All selected$/);
    });
    expect(announcer()).toHaveTextContent(/^All selected$/);
    must(gate.release)();
    await eventually(() => {
      expect(summary()).toHaveTextContent(`All ${String(sentTotal)} selected`);
    });
  });

  it("one row left of a list says how many are selected, not that the list has one row", async () => {
    const env = renderInbox();
    const listBox = await focusedList(env);
    await pressSelectAll(listBox);
    const selection = listSelectionFor(env.store).getState();
    const all = must(selection.all);
    // Leave out every conversation but one (the core counts them).
    const threads = [...new Set(inboxMessages(env).map((m) => m.thread_id))];
    act(() => {
      listSelectionFor(env.store)
        .getState()
        .setExcept(
          threads.slice(1).map((thread, n) => ({
            key: `left-${String(n)}`,
            messageId: must(inboxMessages(env).find((m) => m.thread_id === thread)).id,
            threadId: thread,
            child: false,
          })),
        );
    });
    expect(all.list).toEqual(must(listSelectionFor(env.store).getState().all).list);
    await eventually(() => {
      expect(summary()).toHaveTextContent(/^1 selected$/);
    });
  });
});
