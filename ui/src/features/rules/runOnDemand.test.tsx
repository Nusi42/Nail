/**
 * R1 (ADR 0219): running rules on demand — the "Run rule" menus and their
 * toast, the run helpers and the compact problem indicator. The rules
 * window's own running (R2: the footer's "Run on") is in
 * `./rulesWindow.test.tsx`.
 */
import { act, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expandTargets, listSelectionFor } from "../../actions/mailTargets";
import type { Rule, RuleRun } from "../../api/generated";
import { MOCK_CONVERSATION_ID, addMockConversation } from "../../api/mockConversations";
import { renderWithProviders, type Env } from "../../test/harness";
import { actionContext, requestsOf } from "../../test/env";
import { INBOX } from "../../test/mockIds";
import { must } from "../../test/must";
import { literalMenuLabel, menuStoreFor, type ContextTarget } from "../menus";
import { actions } from "./index";
import { Issues } from "./Issues";
import { ALL_RULES, runMenuItems, runMenuSource } from "./runMenu";
import { mayUndo, outcomeOf } from "./runTargets";
import { RunToast } from "./RunToast";
import { rulesStore } from "./store";
import { addTagAllRule, runsFinished, setup } from "./testing";

function target(part: string, fields: Partial<ContextTarget>): ContextTarget {
  return {
    part,
    element: document.createElement("div"),
    messageId: null,
    threadId: null,
    folderId: null,
    accountId: null,
    attachment: null,
    child: false,
    card: false,
    ...fields,
  };
}

/** Opens a context menu on `on`, as the menu host does. */
function openMenu(env: Env, on: ContextTarget) {
  menuStoreFor(env.store)
    .getState()
    .show({
      menu: { name: `context:${on.part}`, items: [] },
      target: on,
      x: 10,
      y: 20,
      returnFocus: null,
      keyboard: true,
    });
}

describe("the Run rule menus", () => {
  const ctxOf = (env: Env) => actionContext(env);
  const runMenu = must(actions["rules.run-menu"]);
  const run = must(actions["rules.run"]);

  it("lists all rules that are on, the rules that are on, then the rules that are off", () => {
    const rule = (id: string, enabled: boolean) => ({ id, name: id, enabled }) as Rule;
    const labels = runMenuItems([rule("a", true), rule("b", false), rule("c", true)]).map((e) =>
      e.type === "action"
        ? `${JSON.stringify(e.args[0])}:${String(e.label)}`
        : e.type === "header"
          ? e.label
          : e.type,
    );
    expect(labels).toEqual([
      `"${ALL_RULES}":@rules.menu.all`,
      "separator",
      '"a":a',
      '"c":c',
      "separator",
      "@rules.menu.off",
      '"b":b',
    ]);
    // One rule: no "all"; no rules: open the rules window.
    expect(runMenuItems([rule("a", true)])).toHaveLength(1);
    expect(runMenuItems([])).toMatchObject([{ action: "rules.open", label: "@rules.menu.none" }]);
    // R2: a rule name is shown as written, never read as a Fluent id.
    expect(runMenuItems([rule("@home", true)])).toMatchObject([
      { label: literalMenuLabel("@home") },
    ]);
  });

  it("runs a rule on the folder of the folder menu and shows what it did", async () => {
    const env = setup();
    addTagAllRule(env, false);
    const ctx = ctxOf(env);
    expect(runMenu.enabled?.(ctx, [])).toBe(false);
    openMenu(env, target("folder-tree.folder", { folderId: INBOX, accountId: "acc-personal" }));
    expect(runMenu.enabled?.(ctx, [])).toBe(true);

    // R2: the rules are a submenu (a menu source); the open menu stays.
    const before = menuStoreFor(env.store).getState().open;
    await runMenu.run(ctx, []);
    expect(menuStoreFor(env.store).getState().open).toBe(before);
    expect(await runMenuSource.load(ctx)).toMatchObject([
      { action: "rules.run", args: ["rust-announcements"], label: "Rust announcements" },
      { type: "separator" },
      { type: "header", label: "@rules.menu.off" },
      { action: "rules.run", args: ["tag-all"], label: "Tag all" },
    ]);

    await run.run(ctx, ["tag-all"]);
    expect(requestsOf(env, "RunRules")).toEqual([
      { type: "RunRules", rule_ids: ["tag-all"], message_ids: [], folder_id: INBOX },
    ]);
    expect(rulesStore(env.client).getState().toast).toMatchObject({ kind: "ran" });
  });

  it("runs every rule that is on on the selected conversation's messages", async () => {
    const env = setup();
    addTagAllRule(env);
    const added = addMockConversation(env.transport.data);
    const first = must(added.find((m) => m.folder_id === INBOX));
    listSelectionFor(env.store).getState().setScope({ conversations: true, folderId: INBOX });
    openMenu(
      env,
      target("message-list.row", { messageId: first.id, threadId: MOCK_CONVERSATION_ID }),
    );
    await run.run(ctxOf(env), [ALL_RULES]);
    const [request] = requestsOf(env, "RunRules");
    expect(request?.rule_ids).toEqual(["rust-announcements", "tag-all"]);
    const inInbox = added.filter((m) => m.folder_id === INBOX).map((m) => m.id);
    expect(request?.message_ids).toEqual(expect.arrayContaining(inInbox));
    expect(request?.folder_id).toBeNull();
  });

  it("shows the core's refusal in the toast", async () => {
    const env = setup();
    openMenu(env, target("folder-tree.folder", { folderId: "acc-personal/Nowhere" }));
    await run.run(ctxOf(env), ["rust-announcements"]);
    expect(rulesStore(env.client).getState().toast).toMatchObject({
      kind: "error",
      id: "error-not-found",
    });
  });

  it("does nothing on other parts", async () => {
    const env = setup();
    openMenu(env, target("nav-rail", {}));
    expect(runMenu.enabled?.(ctxOf(env), [])).toBe(false);
    await expect(runMenuSource.load(ctxOf(env))).resolves.toEqual([]);
    expect(run.enabled?.(ctxOf(env), ["rust-announcements"])).toBe(false);
  });
});

describe("the run toast", () => {
  it("says what a menu's run did and undoes all of it", async () => {
    const env = setup();
    addTagAllRule(env);
    const user = userEvent.setup({ delay: null });
    renderWithProviders(<RunToast />, env);
    openMenu(env, target("folder-tree.folder", { folderId: INBOX }));
    await act(() => must(actions["rules.run"]).run(actionContext(env), ["tag-all"]));
    // Test DOMs hide a popover they cannot show (the browser shows it in the top layer).
    const toast = await screen.findByRole("status", { hidden: true });
    expect(toast).toHaveTextContent(/^The rule ran: \d+ messages matched, \d+ changes queued\./);
    await runsFinished(env);
    await user.click(within(toast).getByRole("button", { name: "Undo", hidden: true }));
    expect(await screen.findByText("Everything the run did was undone.")).toBeInTheDocument();
    expect(env.transport.data.messages.some((m) => m.tags.includes("picked"))).toBe(false);
    await user.click(screen.getByRole("button", { name: "Dismiss", hidden: true }));
    expect(rulesStore(env.client).getState().toast).toBeNull();
  });
});

describe("run helpers", () => {
  const runOf = (fields: Partial<RuleRun>): RuleRun => ({
    id: "r",
    rule_id: "x",
    rule_name: "X",
    cause: "manual",
    state: "done",
    started_at: "",
    finished_at: null,
    target: { kind: "scope" },
    matched: 0,
    op_ids: [],
    undoable: false,
    message: null,
    ...fields,
  });

  it("sums a click's runs and keeps those whose undo can take something back", () => {
    const done = runOf({ id: "a", matched: 2, op_ids: ["o1", "o2"], undoable: true });
    const finishing = runOf({ id: "b", state: "running", matched: 1, op_ids: ["o3"] });
    const sent = runOf({ id: "c", matched: 1, op_ids: ["o4"] });
    expect(outcomeOf([done, finishing, sent])).toEqual({
      rules: 3,
      matched: 4,
      changes: 4,
      undoable: [done, finishing],
    });
    expect(mayUndo(runOf({ state: "running" }))).toBe(false);
  });

  it("names each message of the targets once", async () => {
    const env = setup();
    const added = addMockConversation(env.transport.data);
    const found = await expandTargets(env.client, [
      { kind: "message", message_id: must(added[0]).id },
      { kind: "conversation", thread_id: MOCK_CONVERSATION_ID, folder_id: null },
    ]);
    const ids = found.map((m) => m.id);
    expect(ids[0]).toBe(must(added[0]).id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("renders no indicator without problems", () => {
    const env = setup();
    const { container } = renderWithProviders(<Issues kind="error" lines={[]} />, env);
    expect(container.querySelector('[data-part="problem"]')).toBeNull();
  });
});
