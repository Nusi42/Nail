/**
 * What a mail action acts on (P3, ADR 0164): the selected conversation, a
 * card's message, a Design B child row, picked rows.
 */
import { createUiStore } from "../state/uiStore";
import { menuStoreFor } from "../features/menus";
import type { ContextTarget } from "../features/menus";
import {
  currentTargets,
  listSelectionFor,
  pickedOf,
  prunePicked,
  rangePick,
  rowTarget,
  togglePick,
  type PickableRow,
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
