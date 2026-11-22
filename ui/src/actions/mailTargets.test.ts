/**
 * What a mail action acts on (P3, ADR 0164): the selected conversation, a
 * card's message, a Design B child row, picked rows.
 */
import { createUiStore } from "../state/uiStore";
import { menuStoreFor } from "../features/menus";
import type { ContextTarget } from "../features/menus";
import type { MailList } from "../api/generated";
import {
  currentTargets,
  hasListTarget,
  inAll,
  listSelectionFor,
  listTargetOf,
  loadedRowTargets,
  mailListOf,
  rangeExcept,
  sameList,
  toggleExcept,
  withAsOf,
  pickedOf,
  prunePicked,
  rangePick,
  rowTarget,
  togglePick,
  type PickableRow,
  refetchedSince,
} from "./mailTargets";

const INBOX = "acc/Inbox";

const row = (n: number, kind: PickableRow["kind"] = "conversation"): PickableRow => ({
  key: kind === "child" ? `t${String(n)}/m${String(n)}` : `t${String(n)}`,
  kind,
  messageId: `m${String(n)}`,
  threadId: kind === "child" ? null : `t${String(n)}`,
});

function menuOn(store: ReturnType<typeof createUiStore>, target: Partial<ContextTarget>) {
  menuStoreFor(store)
    .getState()
    .show({
      menu: { name: "context:x", items: [] },
      target: {
        part: "message-list.row",
        element: document.body,
        messageId: null,
        threadId: null,
        folderId: null,
        accountId: null,
        attachment: null,
        child: false,
        card: false,
        ...target,
      },
      x: 0,
      y: 0,
      returnFocus: null,
      keyboard: false,
    });
}

describe("mail action targets", () => {
  it("a conversation row stands for its conversation in the listed folder", () => {
    const scope = { conversations: true, folderId: INBOX };
    expect(rowTarget({ messageId: "m1", threadId: "t1", child: false }, scope)).toEqual({
      kind: "conversation",
      thread_id: "t1",
      folder_id: INBOX,
    });
    // A Design B child row and a list without conversations: the message.
    expect(rowTarget({ messageId: "m1", threadId: null, child: true }, scope)).toEqual({
      kind: "message",
      message_id: "m1",
    });
    expect(
      rowTarget(
        { messageId: "m1", threadId: "t1", child: false },
        { conversations: false, folderId: INBOX },
      ),
    ).toEqual({ kind: "message", message_id: "m1" });
    // While searching: the conversation everywhere (no folder).
    expect(
      rowTarget(
        { messageId: "m1", threadId: "t1", child: false },
        { conversations: true, folderId: null },
      ),
    ).toEqual({ kind: "conversation", thread_id: "t1", folder_id: null });
  });

  it("the selection is the selected conversation, also while a card is selected", () => {
    const store = createUiStore();
    expect(currentTargets(store)).toEqual({ targets: [], source: "none" });
    listSelectionFor(store).getState().setScope({ conversations: true, folderId: INBOX });
    // The reader selected an older card: still the conversation.
    store.getState().selectMessage("older-card");
    store.getState().selectThread("t1");
    expect(currentTargets(store)).toEqual({
      targets: [{ kind: "conversation", thread_id: "t1", folder_id: INBOX }],
      source: "list",
    });
    // A Design B child row selects no thread: its message.
    store.getState().selectThread(null);
    expect(currentTargets(store).targets).toEqual([{ kind: "message", message_id: "older-card" }]);
  });

  it("a card's menu acts on its message, a row's menu on its conversation", () => {
    const store = createUiStore();
    listSelectionFor(store).getState().setScope({ conversations: true, folderId: INBOX });
    store.getState().selectMessage("m9");
    store.getState().selectThread("t9");
    menuOn(store, { part: "thread-view.message", messageId: "m2", card: true });
    expect(currentTargets(store)).toEqual({
      targets: [{ kind: "message", message_id: "m2" }],
      source: "card",
    });
    menuOn(store, { messageId: "m3", threadId: "t3" });
    expect(currentTargets(store).targets).toEqual([
      { kind: "conversation", thread_id: "t3", folder_id: INBOX },
    ]);
    menuOn(store, { messageId: "m4", threadId: "t4", child: true });
    expect(currentTargets(store).targets).toEqual([{ kind: "message", message_id: "m4" }]);
    menuStoreFor(store).getState().close(false);
  });

  it("picked rows act together, also from the menu of one of them", () => {
    const store = createUiStore();
    const selection = listSelectionFor(store);
    selection.getState().setScope({ conversations: true, folderId: INBOX });
    selection.getState().setPicked([pickedOf(row(1)), pickedOf(row(2)), pickedOf(row(3, "child"))]);
    const all = [
      { kind: "conversation", thread_id: "t1", folder_id: INBOX },
      { kind: "conversation", thread_id: "t2", folder_id: INBOX },
      { kind: "message", message_id: "m3" },
    ];
    expect(currentTargets(store).targets).toEqual(all);
    menuOn(store, { messageId: "m2", threadId: "t2" });
    expect(currentTargets(store).targets).toEqual(all);
    // A row outside the picked ones: that row alone.
    menuOn(store, { messageId: "m7", threadId: "t7" });
    expect(currentTargets(store).targets).toEqual([
      { kind: "conversation", thread_id: "t7", folder_id: INBOX },
    ]);
    menuStoreFor(store).getState().close(false);
    // Another folder drops the picked rows; one picked row is no pick.
    selection.getState().setScope({ conversations: true, folderId: "acc/Archive" });
    expect(selection.getState().picked).toEqual([]);
    selection.getState().setPicked([pickedOf(row(1))]);
    expect(selection.getState().picked).toEqual([]);
  });

  it("Ctrl + click toggles a row, Shift + click picks a range", () => {
    const rows = [row(1), row(2), row(3), row(4)];
    expect(togglePick([], rows[0], row(3)).map((p) => p.key)).toEqual(["t1", "t3"]);
    const two = togglePick([], rows[0], row(3));
    expect(togglePick(two, rows[0], row(3)).map((p) => p.key)).toEqual(["t1"]);
    expect(togglePick([], undefined, row(2)).map((p) => p.key)).toEqual(["t2"]);
    expect(rangePick(rows, 3, 1).map((p) => p.key)).toEqual(["t2", "t3", "t4"]);
    expect(rangePick(rows, -1, 2).map((p) => p.key)).toEqual(["t3"]);
    expect(pickedOf(row(5, "child"))).toEqual({
      key: "t5/m5",
      messageId: "m5",
      threadId: null,
      child: true,
    });
    expect(prunePicked(rangePick(rows, 0, 3), [{ key: "t2" }, { key: "t4" }])).toHaveLength(2);
  });
});

describe("the selection after a resync (ADR 0230)", () => {
  it("waits for data fetched after the resync and for nothing loading", () => {
    expect(refetchedSince(null, { dataUpdatedAt: 5_000, isFetching: false })).toBe(false);
    // Data from before the resync, or a refetch still running: not yet.
    expect(refetchedSince(4_000, { dataUpdatedAt: 3_000, isFetching: false })).toBe(false);
    expect(refetchedSince(4_000, { dataUpdatedAt: 5_000, isFetching: true })).toBe(false);
    expect(refetchedSince(4_000, { dataUpdatedAt: 4_000, isFetching: false })).toBe(true);
    expect(refetchedSince(4_000, { dataUpdatedAt: 6_000, isFetching: false })).toBe(true);
  });
});

describe("select all (R5, ADR 0235)", () => {
  const LIST: MailList = { source: "conversations", folder_id: INBOX };
  const SCOPE = { conversations: true, folderId: INBOX };

  it("names the list the message list pages: a folder's conversations or messages, or its search", () => {
    const base = {
      search: null,
      folderId: INBOX,
      threadRows: true,
      conversations: true,
      pageSize: 50,
    };
    expect(mailListOf(base)).toEqual(LIST);
    expect(mailListOf({ ...base, threadRows: false, conversations: false })).toEqual({
      source: "messages",
      folder_id: INBOX,
    });
    expect(mailListOf({ ...base, folderId: null })).toBeNull();
    const search = { text: "from:ada is:unread", accountId: "acc", folderId: INBOX };
    expect(mailListOf({ ...base, search })).toEqual({
      source: "search",
      query: {
        text: "from:ada is:unread",
        account_id: "acc",
        folder_id: INBOX,
        offset: 0,
        limit: 50,
        sort: "date",
      },
      conversations: true,
    });
  });

  it("Ctrl+A selects the whole list; actions get one list target; Esc and another list end it", () => {
    const store = createUiStore();
    const selection = listSelectionFor(store);
    selection.getState().setScope(SCOPE);
    // No list shown yet: nothing to select.
    selection.getState().selectAll();
    expect(selection.getState().all).toBeNull();
    selection.getState().setList(LIST);
    selection.getState().setPicked([pickedOf(row(1)), pickedOf(row(2))]);
    selection.getState().selectAll();
    expect(selection.getState().picked).toEqual([]);
    expect(currentTargets(store)).toEqual({
      targets: [{ kind: "list", list: LIST, except: [], as_of: null }],
      source: "list",
    });
    // Left out rows travel as the targets they stand for.
    selection.getState().setExcept([pickedOf(row(2)), pickedOf(row(3, "child"))]);
    expect(currentTargets(store).targets).toEqual([
      {
        kind: "list",
        list: LIST,
        except: [
          { kind: "conversation", thread_id: "t2", folder_id: INBOX },
          { kind: "message", message_id: "m3" },
        ],
        as_of: null,
      },
    ]);
    // A selected row's menu acts on the selection, a left-out row's on itself.
    menuOn(store, { messageId: "m1", threadId: "t1" });
    expect(hasListTarget(currentTargets(store).targets)).toBe(true);
    menuOn(store, { messageId: "m2", threadId: "t2" });
    expect(currentTargets(store).targets).toEqual([
      { kind: "conversation", thread_id: "t2", folder_id: INBOX },
    ]);
    menuStoreFor(store).getState().close(false);
    // The same list again (a new object) keeps it; another list ends it.
    selection.getState().setList({ ...LIST });
    expect(selection.getState().all).not.toBeNull();
    selection.getState().setList({ source: "conversations", folder_id: "acc/Archive" });
    expect(selection.getState().all).toBeNull();
    // Esc.
    selection.getState().setList(LIST);
    selection.getState().selectAll();
    selection.getState().clear();
    expect(selection.getState().all).toBeNull();
    // A plain click (an explicit selection) ends it too.
    selection.getState().selectAll();
    selection.getState().setPicked([]);
    expect(selection.getState().all).toBeNull();
    expect(sameList(LIST, { ...LIST })).toBe(true);
  });

  it("Ctrl / Shift + click leave rows out and take them back", () => {
    const rows = [row(1), row(2), row(3), row(4), row(5)];
    const one = toggleExcept([], row(2));
    expect(one.map((p) => p.key)).toEqual(["t2"]);
    expect(toggleExcept(one, row(2))).toEqual([]);
    const all = { list: LIST, except: one };
    expect(inAll(all, "t1")).toBe(true);
    expect(inAll(all, "t2")).toBe(false);
    // Shift + click on a selected row leaves the range out ...
    const range = rangeExcept(one, rows, 2, 4);
    expect(range.map((p) => p.key).sort()).toEqual(["t2", "t3", "t4", "t5"]);
    // ... on a left-out row takes it back.
    expect(rangeExcept(range, rows, 4, 2).map((p) => p.key)).toEqual(["t2"]);
  });

  it("updates the loaded rows of a list target at once, and sends the confirmation's mark", () => {
    const target = listTargetOf({ list: LIST, except: [pickedOf(row(2))] }, SCOPE);
    const order = ["m1", "m2", "m3"];
    const threads = { m1: "t1", m2: "t2", m3: "t3" };
    expect(loadedRowTargets(target, order, threads, SCOPE)).toEqual([
      { kind: "conversation", thread_id: "t1", folder_id: INBOX },
      { kind: "conversation", thread_id: "t3", folder_id: INBOX },
    ]);
    const message = { kind: "message", message_id: "m9" } as const;
    expect(loadedRowTargets(message, order, threads, SCOPE)).toEqual([message]);
    expect(withAsOf([target, message], "mark")).toEqual([{ ...target, as_of: "mark" }, message]);
    expect(hasListTarget([message])).toBe(false);
  });
});
