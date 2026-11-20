/**
 * R1 (ADR 0219): running rules on demand — the run panel of the rules
 * window, the "Run rule" menus and their toast, the compact problem
 * indicator, and the Editor / Text switch of a new rule.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { expandTargets, listSelectionFor, NO_LIST_SCOPE } from "../../actions/mailTargets";
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
import { runPlan, type EditedRule } from "./RunPanel";
import { mayUndo, outcomeOf } from "./runTargets";
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

/** A manual rule that flags every message, as `rules.kdl` text. */
const FLAG_THEM = `rule "Flag them" trigger="manual" {
    then {
        flag
    }
}
`;

/**
 * Renders the rules window and opens the editor with the button named
 * `button` ("New rule", or a rule's name in the list).
 */
async function openEditor(env: Env, button: string) {
  const user = userEvent.setup({ delay: null });
  renderWithProviders(<RuleManagerView />, env);
  await user.click(await screen.findByRole("button", { name: button }));
  const editor = await screen.findByRole("form", { name: "Rule" });
  return { user, editor, panel: within(editor).getByRole("region", { name: "Run rules" }) };
}

/** Opens a new rule in the text view, with `FLAG_THEM` typed into it. */
async function newRuleAsText() {
  const env = setup({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
  const opened = await openEditor(env, "New rule");
  const source = await showTextView(opened.user, opened.editor);
  await opened.user.clear(source);
  await opened.user.paste(FLAG_THEM);
  return { ...opened, env, source };
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
    const { user, panel } = await openEditor(env, "Rust announcements");
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
    expect(within(panel).getByText(/Choose a rule/)).toBeInTheDocument();
    expect(within(panel).getByRole("button", { name: "Run now" })).toBeDisabled();
    expect(within(panel).getByRole("button", { name: "Undo" })).toBeDisabled();
  });

  it("saves an edited rule before running it, never the old saved one", async () => {
    const env = setup({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    const { user, editor, panel } = await openEditor(env, "Rust announcements");
    // Saved and unchanged: it runs as saved.
    expect(within(panel).getByRole("button", { name: "Run now" })).toBeEnabled();
    const name = within(editor).getByRole("textbox", { name: "Name" });
    await user.clear(name);
    await user.type(name, "Rust news");
    expect(within(panel).getByText("Saves “Rust news”, then runs it.")).toBeInTheDocument();
    await user.click(within(panel).getByRole("radio", { name: "The open folder “Inbox”" }));
    await user.click(within(panel).getByRole("button", { name: "Save and run" }));
    await within(panel).findByText(/^The rule ran:/);
    const saved = must(requestsOf(env, "SaveRule")[0]);
    expect(saved.rule.name).toBe("Rust news");
    expect(requestsOf(env, "RunRules")).toEqual([
      { type: "RunRules", rule_ids: ["rust-announcements"], message_ids: [], folder_id: INBOX },
    ]);
    // Saved now: the next click runs it as it is.
    expect(within(panel).getByRole("button", { name: "Run now" })).toBeEnabled();
  });

  it("saves a new rule and runs it from its editor", async () => {
    const env = setup({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    const { user, editor, panel } = await openEditor(env, "New rule");
    // Nothing to do yet: the reason is shown, the button waits.
    expect(within(panel).getByText(/Fix what the rule still misses/)).toBeInTheDocument();
    expect(within(panel).getByRole("button", { name: "Save and run" })).toBeDisabled();
    await user.type(within(editor).getByRole("textbox", { name: "Name" }), "Fresh");
    await user.selectOptions(within(editor).getByRole("combobox", { name: "Action" }), "archive");
    await user.click(within(editor).getByRole("button", { name: "Add action" }));
    await user.click(within(panel).getByRole("button", { name: "Save and run" }));
    await within(panel).findByText(/^The rule ran:/);
    const [request] = requestsOf(env, "RunRules");
    const created = env.transport.data.features.rules.rules.find((r) => r.name === "Fresh");
    expect(request?.rule_ids).toEqual([must(created).id]);
  });

  it("names the rule typed in a new rule's text view, as Save would write it", async () => {
    const { user, panel, source } = await newRuleAsText();
    // Read once typing pauses: the name of the rule in the text, not "".
    expect(await within(panel).findByText("Saves “Flag them”, then runs it.")).toBeInTheDocument();
    expect(within(panel).getByRole("button", { name: "Save and run" })).toBeEnabled();
    // Text that does not read cannot be saved, so the run waits for a fix.
    await user.clear(source);
    await user.paste('rule "Broken" {');
    expect(await within(panel).findByText(/Fix what the rule still misses/)).toBeInTheDocument();
    expect(within(panel).getByRole("button", { name: "Save and run" })).toBeDisabled();
  });

  it("runs a rule saved from the text view as saved, without saving it again", async () => {
    const { env, user, editor, panel, source } = await newRuleAsText();
    await user.click(within(editor).getByRole("button", { name: "Save rule" }));
    await within(editor).findByText("The rule is saved.");
    // The text shows the saved rule, so nothing is left to save.
    expect(await within(panel).findByText("Runs “Flag them”.")).toBeInTheDocument();
    expect(source.value).toContain('rule "Flag them"');
    await user.click(within(panel).getByRole("radio", { name: "The open folder “Inbox”" }));
    await user.click(within(panel).getByRole("button", { name: "Run now" }));
    await within(panel).findByText(/^The rule ran:/);
    expect(requestsOf(env, "SaveRule")).toHaveLength(1);
  });

  it("offers no folder while the list shows search results", async () => {
    const env = setup({ state: { activeAccountId: "acc-personal", activeFolderId: INBOX } });
    act(() => {
      env.store.getState().setSearch({
        text: "is:flagged",
        accountId: null,
        folderId: null,
        error: null,
      });
    });
    renderWithProviders(<RuleManagerView />, env);
    const folder = await within(rulesRunPanel()).findByRole("radio", { name: /The open folder/ });
    expect(folder).toBeDisabled();
    expect(folder).toHaveAccessibleDescription(
      "Not while the list shows search results: open a folder.",
    );
  });

  it("names the list's columns, so the run and on check boxes are told apart", async () => {
    const env = setup();
    const { container } = renderWithProviders(<RuleManagerView />, env);
    await screen.findByRole("button", { name: "Rust announcements" });
    const head = must(container.querySelector('[data-part="rules.list.head"]'));
    expect(head).toHaveAttribute("aria-hidden", "true");
    expect(
      [...head.querySelectorAll('[data-part="rules.list.head.cell"]')].map((c) => c.textContent),
    ).toEqual(["Run", "Rule", "On"]);
    const row = must(container.querySelector('[data-part="rules.list.item"]'));
    // Pick box, the rule, the on box, Up, Down: one control per header column.
    expect([...row.children].map((c) => c.getAttribute("data-part"))).toEqual([
      "form.checkbox",
      "rules.list.main",
      "form.check",
      "form.button",
      "form.button",
    ]);
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
    const { user, editor } = await openEditor(env, "New rule");
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
  const edited = (fields: Partial<EditedRule>): EditedRule => ({
    id: "a",
    name: "A",
    unsaved: false,
    blocked: false,
    save: () => Promise.resolve("a"),
    ...fields,
  });

  it("plans a click: the checked rules, else the edited one, saved first when changed", () => {
    expect(runPlan([], null)).toEqual({ ids: [], saveFirst: false });
    expect(runPlan([], edited({}))).toEqual({ ids: ["a"], saveFirst: false });
    expect(runPlan([], edited({ unsaved: true }))).toEqual({ ids: [], saveFirst: true });
    expect(runPlan([], edited({ id: null, unsaved: true }))).toEqual({ ids: [], saveFirst: true });
    // Checked rules run; the edited one is saved first only when it is one of them.
    expect(runPlan(["b"], edited({ unsaved: true }))).toEqual({ ids: ["b"], saveFirst: false });
    expect(runPlan(["b", "a"], edited({ unsaved: true }))).toEqual({
      ids: ["b", "a"],
      saveFirst: true,
    });
  });

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
