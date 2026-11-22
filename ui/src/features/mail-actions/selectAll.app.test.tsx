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
import { INBOX } from "../../test/mockIds";
import { listSelectionFor } from "../../actions/mailTargets";
import { BULK_CONFIRM_DIALOG } from "./bulk";
import { CLEAR_SELECTION_ACTION, SELECT_ALL_ACTION } from "./index";

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

    fireEvent.keyDown(listBox, { key: "a", ctrlKey: true });
    await eventually(() => {
      expect(summary()).toHaveTextContent(`All ${String(total)} selected`);
    });
    expect(summary()).toHaveAttribute("aria-live", "polite");
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
});
