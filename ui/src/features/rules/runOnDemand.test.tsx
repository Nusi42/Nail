/**
 * R1 (ADR 0219): running rules on demand — the run panel of the rules
 * window, the "Run rule" menus and their toast, the compact problem
 * indicator, and the Editor / Text switch of a new rule.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { listSelectionFor, NO_LIST_SCOPE } from "../../actions/mailTargets";
import type { Rule, RuleRun } from "../../api/generated";
import { MOCK_CONVERSATION_ID, addMockConversation } from "../../api/mockConversations";
import { createEnv, renderWithProviders, type Env } from "../../test/harness";
import { actionContext, requestsOf } from "../../test/env";
import { INBOX } from "../../test/mockIds";
import { must } from "../../test/must";
import { menuStoreFor, type ContextTarget } from "../menus";
import { onRulesEvent } from "./api";
import { actions } from "./index";
import { Issues } from "./Issues";
import { RuleManagerView } from "./RuleManager";
import { ALL_RULES, RUN_MENU_NAME, runMenuItems } from "./runMenu";
import { mayUndo, messageIdsOf, outcomeOf } from "./runTargets";
import { RunToast } from "./RunToast";
import { rulesStore } from "./store";
import { showTextView } from "./testing";

const rulesRunPanel = () => screen.getByRole("region", { name: "Run rules" });

function setup(state: Parameters<typeof createEnv>[0] = {}): Env {
  const env = createEnv(state);
  env.transport.data.behaviour.workMs = 0;
  env.transport.onEvent((event) => {
    onRulesEvent(event, env.queryClient);
  });
  return env;
}

/** A second rule that tags every message. */
function addTagAllRule(env: Env, enabled = true): Rule {
  const rule: Rule = {
    id: "tag-all",
    name: "Tag all",
    enabled,
    triggers: [],
    scope: [],
    conditions: { type: "all", conditions: [] },
    actions: [{ type: "tag", tag: "picked" }],
    source: null,
  };
  env.transport.data.features.rules.rules.push(rule);
  return rule;
}

/** Every run the mock recorded is finished (undo needs that). */
async function runsFinished(env: Env) {
  await waitFor(() => {
    expect(env.transport.data.features.rules.runs.every((r) => r.state === "done")).toBe(true);
  });
}

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

describe("the rules window's run panel", () => {
  it("runs the checked rules on the selected messages and undoes the click", async () => {
    const env = setup();
    addTagAllRule(env);
    const [a, b] = env.transport.data.messages.filter((m) => m.folder_id === INBOX);
    const rows = [must(a), must(b)].map((m) => ({
      key: m.id,
      messageId: m.id,
      threadId: null,
      child: false,
    }));
    listSelectionFor(env.store).getState().setScope(NO_LIST_SCOPE);
    listSelectionFor(env.store).getState().setPicked(rows);
    env.store.getState().selectMessage(must(a).id);
    const user = userEvent.setup({ delay: null });
    renderWithProviders(<RuleManagerView />, env);

    await user.click(await screen.findByRole("checkbox", { name: /Check “Tag all”/ }));
    await user.click(screen.getByRole("checkbox", { name: /Check “Rust announcements”/ }));
    const panel = rulesRunPanel();
    expect(within(panel).getByText("Runs the 2 checked rules.")).toBeInTheDocument();
    await user.click(within(panel).getByRole("radio", { name: "The selected messages (2)" }));
    await user.click(within(panel).getByRole("button", { name: "Run now" }));

    expect(await within(panel).findByRole("status")).toHaveTextContent(
      "2 rules ran: 2 messages matched, 2 changes queued.",
    );
    expect(requestsOf(env, "RunRules")).toEqual([
      {
        type: "RunRules",
        rule_ids: ["tag-all", "rust-announcements"],
        message_ids: [must(a).id, must(b).id],
        folder_id: null,
      },
    ]);
    expect(env.transport.data.messages.filter((m) => m.tags.includes("picked"))).toHaveLength(2);

    await runsFinished(env);
    await user.click(within(panel).getByRole("button", { name: "Undo" }));
    expect(await within(panel).findByText("Everything the run did was undone.")).toBeVisible();
    // Only the run that changed something is undone.
    expect(requestsOf(env, "UndoRuleRun")).toHaveLength(1);
    expect(env.transport.data.messages.some((m) => m.tags.includes("picked"))).toBe(false);
  });

  it("runs the rule being edited on the open folder", async () => {
    const env = setup({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    const user = userEvent.setup({ delay: null });
    renderWithProviders(<RuleManagerView />, env);
    await user.click(await screen.findByRole("button", { name: "Rust announcements" }));
    const panel = rulesRunPanel();
    await user.click(await within(panel).findByRole("radio", { name: "The open folder “Inbox”" }));
    await user.click(within(panel).getByRole("button", { name: "Run now" }));
    await within(panel).findByText(/^The rule ran:/);
    expect(requestsOf(env, "RunRules")).toEqual([
      { type: "RunRules", rule_ids: ["rust-announcements"], message_ids: [], folder_id: INBOX },
    ]);
  });

  it("says why a target is not available and what it would run", async () => {
    const env = setup();
    renderWithProviders(<RuleManagerView />, env);
    const panel = rulesRunPanel();
    const messages = await within(panel).findByRole("radio", { name: "The selected messages" });
    expect(messages).toBeDisabled();
    expect(messages).toHaveAccessibleDescription("Select messages in the list first.");
    expect(within(panel).getByRole("radio", { name: /The open folder/ })).toBeDisabled();
    // Nothing chosen yet: nothing to run.
    expect(within(panel).getByText(/Choose a saved rule/)).toBeInTheDocument();
    expect(within(panel).getByRole("button", { name: "Run now" })).toBeDisabled();
    expect(within(panel).getByRole("button", { name: "Undo" })).toBeDisabled();
  });

  it("forgets a deleted rule that was checked", async () => {
    const env = setup();
    addTagAllRule(env);
    const user = userEvent.setup({ delay: null });
    renderWithProviders(<RuleManagerView />, env);
    await user.click(await screen.findByRole("checkbox", { name: /Check “Tag all”/ }));
    expect(rulesStore(env.client).getState().picked).toEqual(["tag-all"]);
    await user.click(screen.getByRole("button", { name: "Tag all" }));
    await user.click(await screen.findByRole("button", { name: "Delete rule" }));
    await waitFor(() => {
      expect(rulesStore(env.client).getState().picked).toEqual([]);
    });
  });
});

describe("the editor's view switch", () => {
  it("goes Editor → Text → Editor on a new rule without reading the text back", async () => {
    const env = setup();
    const user = userEvent.setup({ delay: null });
    renderWithProviders(<RuleManagerView />, env);
    await user.click(await screen.findByRole("button", { name: "New rule" }));
    const editor = await screen.findByRole("form", { name: "Rule" });
    await user.type(within(editor).getByRole("textbox", { name: "Name" }), "Fresh");
    const source = await showTextView(user, editor);
    expect(source.value).toContain('rule "Fresh"');
    // The core writes a new rule's empty id into the text (`id=""`), which
    // does not read back: unchanged text must not be parsed.
    await user.click(within(editor).getByRole("button", { name: "Editor" }));
    expect(await within(editor).findByRole("textbox", { name: "Name" })).toHaveValue("Fresh");
    expect(requestsOf(env, "RuleFromText")).toEqual([]);
    expect(within(editor).queryByRole("alert")).toBeNull();
    // What is missing stays a compact indicator, not a block above the buttons.
    const missing = within(editor).getByRole("button", { name: "One thing to fix" });
    expect(missing).toHaveAccessibleDescription("Add at least one action.");
    expect(missing.closest('[data-part="rules.views"]')).not.toBeNull();
  });
});

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
  });

  it("runs a rule on the folder of the folder menu and shows what it did", async () => {
    const env = setup();
    addTagAllRule(env, false);
    const ctx = ctxOf(env);
    expect(runMenu.enabled?.(ctx, [])).toBe(false);
    openMenu(env, target("folder-tree.folder", { folderId: INBOX, accountId: "acc-personal" }));
    expect(runMenu.enabled?.(ctx, [])).toBe(true);

    await runMenu.run(ctx, []);
    const open = must(menuStoreFor(env.store).getState().open);
    expect(open.menu.name).toBe(RUN_MENU_NAME);
    expect(open.target.folderId).toBe(INBOX);
    expect(open.menu.items).toMatchObject([
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

  it("does nothing on other parts", () => {
    const env = setup();
    openMenu(env, target("nav-rail", {}));
    expect(runMenu.enabled?.(ctxOf(env), [])).toBe(false);
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
    const ids = await messageIdsOf(env.client, [
      { kind: "message", message_id: must(added[0]).id },
      { kind: "conversation", thread_id: MOCK_CONVERSATION_ID, folder_id: null },
    ]);
    expect(ids[0]).toBe(must(added[0]).id);
    expect(new Set(ids).size).toBe(ids.length);
  });

  it("renders no indicator without problems", () => {
    const env = setup();
    const { container } = renderWithProviders(<Issues kind="error" lines={[]} />, env);
    expect(container.querySelector('[data-part="problem"]')).toBeNull();
  });
});
