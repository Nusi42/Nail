/**
 * R3 (ADR 0227): the rule list as a multi-select listbox (`widgets/listSelection.ts`,
 * keys and pointer), Run and Test with several rules, the combined Exchange
 * badge as the row's description, and "Values from this mail" right after
 * a condition's value field.
 */
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Rule } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { renderWithProviders } from "../../test/harness";
import { requestsOf } from "../../test/env";
import { INBOX } from "../../test/mockIds";
import { must } from "../../test/must";
import { setup as exchangeSetup } from "../exchange/testing";
import { newRule } from "./model";
import { RuleManagerView, selectedRules, selectionForKey } from "./RuleManager";
import { runLabel } from "./RuleEditor";
import { gestureOf, inListOrder, pick, pickAll, stepFrom } from "../../widgets/listSelection";
import { rulesStore } from "./store";
import {
  addArchiveAllRule,
  addTagAllRule,
  findQuestion,
  footerOf,
  IN_INBOX,
  openEditor,
  ruleOption,
  runsFinished,
  setup,
} from "./testing";

const ORDER = ["a", "b", "c", "d"];
const NONE = { selected: [], anchor: null };
const key = (
  k: string,
  mods: Partial<Record<"shiftKey" | "ctrlKey" | "metaKey" | "altKey", boolean>> = {},
) => ({
  key: k,
  shiftKey: false,
  ctrlKey: false,
  metaKey: false,
  altKey: false,
  ...mods,
});

describe("the list selection", () => {
  it("reads the gesture from the modifier keys", () => {
    expect(gestureOf({ shiftKey: false, ctrlKey: false, metaKey: false })).toBe("single");
    expect(gestureOf({ shiftKey: false, ctrlKey: true, metaKey: false })).toBe("toggle");
    expect(gestureOf({ shiftKey: false, ctrlKey: false, metaKey: true })).toBe("toggle");
    expect(gestureOf({ shiftKey: true, ctrlKey: true, metaKey: false })).toBe("range");
  });

  it("selects one, toggles, and extends a range from the anchor", () => {
    const one = pick(ORDER, NONE, "b", "single");
    expect(one).toEqual({ selected: ["b"], anchor: "b", focus: "b" });
    const two = pick(ORDER, one, "d", "toggle");
    expect(two).toEqual({ selected: ["b", "d"], anchor: "d", focus: "d" });
    expect(pick(ORDER, two, "b", "toggle")).toEqual({ selected: ["d"], anchor: "b", focus: "b" });
    // A range runs from the anchor, in list order, whichever way it goes.
    expect(pick(ORDER, one, "d", "range")).toEqual({
      selected: ["b", "c", "d"],
      anchor: "b",
      focus: "d",
    });
    expect(pick(ORDER, one, "a", "range").selected).toEqual(["a", "b"]);
    // Moving only the focus keeps the selection and its anchor.
    expect(pick(ORDER, two, "a", "focus")).toEqual({
      selected: ["b", "d"],
      anchor: "d",
      focus: "a",
    });
    // No anchor (or one no longer listed): the range starts at the pick.
    expect(pick(ORDER, { selected: [], anchor: "gone" }, "c", "range")).toEqual({
      selected: ["c"],
      anchor: "c",
      focus: "c",
    });
  });

  it("selects all, steps through the list and keeps the list order", () => {
    expect(pickAll(ORDER, "c")).toEqual({ selected: ORDER, anchor: "c", focus: "c" });
    expect(pickAll(ORDER, null)?.focus).toBe("a");
    expect(pickAll([], null)).toBeNull();
    expect(stepFrom(ORDER, "b", 1)).toBe("c");
    expect(stepFrom(ORDER, "d", 1)).toBe("d");
    expect(stepFrom(ORDER, "a", -1)).toBe("a");
    expect(stepFrom(ORDER, null, 1)).toBe("a");
    expect(inListOrder(ORDER, ["d", "x", "a"])).toEqual(["a", "d"]);
  });

  it("maps the list keys onto the selection", () => {
    const one = { selected: ["b"], anchor: "b" };
    expect(selectionForKey(key("ArrowDown"), ORDER, one, "b")).toEqual({
      selected: ["c"],
      anchor: "c",
      focus: "c",
    });
    expect(selectionForKey(key("ArrowDown", { shiftKey: true }), ORDER, one, "b")).toEqual({
      selected: ["b", "c"],
      anchor: "b",
      focus: "c",
    });
    expect(selectionForKey(key("End", { shiftKey: true }), ORDER, one, "b")?.selected).toEqual([
      "b",
      "c",
      "d",
    ]);
    expect(selectionForKey(key("Home"), ORDER, one, "b")?.focus).toBe("a");
    // Ctrl / ⌘ with an arrow, Home or End moves only the focus.
    expect(selectionForKey(key("ArrowDown", { ctrlKey: true }), ORDER, one, "b")).toEqual({
      selected: ["b"],
      anchor: "b",
      focus: "c",
    });
    expect(selectionForKey(key("End", { metaKey: true }), ORDER, one, "b")).toEqual({
      selected: ["b"],
      anchor: "b",
      focus: "d",
    });
    expect(selectionForKey(key(" "), ORDER, one, "b")?.selected).toEqual([]);
    expect(selectionForKey(key("a", { ctrlKey: true }), ORDER, one, "b")?.selected).toEqual(ORDER);
    expect(selectionForKey(key("A", { metaKey: true }), ORDER, one, "b")?.selected).toEqual(ORDER);
    // Escape: back to the focused rule alone; nothing to do when it is alone.
    expect(
      selectionForKey(key("Escape"), ORDER, { selected: ["a", "b"], anchor: "a" }, "b"),
    ).toEqual({ selected: ["b"], anchor: "b", focus: "b" });
    expect(selectionForKey(key("Escape"), ORDER, one, "b")).toBeNull();
    // Alt+arrows reorder; other keys do nothing here.
    expect(selectionForKey(key("ArrowUp", { altKey: true }), ORDER, one, "b")).toBeNull();
    expect(selectionForKey(key("a"), ORDER, one, "b")).toBeNull();
    expect(selectionForKey(key("ArrowDown"), [], NONE, null)).toBeNull();
  });

  it("acts on the selected rules only when there are several, in file order", () => {
    const rules = ["a", "b", "c"].map((id) => ({ ...newRule(), id, name: id }));
    expect(selectedRules(rules, ["c", "a"]).map((r) => r.id)).toEqual(["a", "c"]);
    expect(selectedRules(rules, ["b"])).toEqual([]);
    // Rules the filter hides are not acted on.
    expect(selectedRules(rules.slice(0, 2), ["a", "c"])).toEqual([]);
  });

  it("labels Run with the number of rules", () => {
    const CASES = [
      { count: 1, savesFirst: false },
      { count: 1, savesFirst: true },
      { count: 3, savesFirst: false },
      { count: 2, savesFirst: true },
    ];
    function Labels() {
      const t = useT();
      return (
        <ul>
          {CASES.map((c) => (
            <li key={`${String(c.count)}-${String(c.savesFirst)}`}>{runLabel(t, c)}</li>
          ))}
        </ul>
      );
    }
    renderWithProviders(<Labels />, setup());
    expect(screen.getAllByRole("listitem").map((li) => li.textContent)).toEqual([
      "Run",
      "Save and run",
      "Run 3 rules",
      "Save and run 2 rules",
    ]);
  });
});

describe("the rules store's selection", () => {
  it("changes the selection at once on the shown rule, else asks first while unsaved", () => {
    const env = setup();
    const store = rulesStore(env.client);
    const asked: (() => void)[] = [];
    store.getState().setLeave((proceed) => {
      asked.push(proceed);
    });
    store.getState().select("a");
    expect(store.getState().selection).toEqual({ selected: ["a"], anchor: "a" });
    store.getState().choose({ selected: ["a", "b"], anchor: "a", focus: "a" });
    expect(store.getState().selection.selected).toEqual(["a", "b"]);
    store.getState().setDirty(true);
    store.getState().choose({ selected: ["a", "b"], anchor: "a", focus: "b" });
    // The editor has changes: nothing moves until the user answers.
    expect(store.getState().selection.selected).toEqual(["a", "b"]);
    expect(store.getState().editing).toMatchObject({ id: "a" });
    expect(asked).toHaveLength(1);
    // Cancel: nothing moves.
    asked.length = 0;
    expect(store.getState().editing).toMatchObject({ id: "a" });
    store.getState().choose({ selected: ["c"], anchor: "c", focus: "c" });
    // Save or Don't save: the pick goes ahead.
    asked.shift()?.();
    expect(store.getState().editing).toMatchObject({ id: "c" });
    expect(store.getState().selection.selected).toEqual(["c"]);
  });

  it("clears the selection for a new rule and selects it once saved", () => {
    const env = setup();
    const store = rulesStore(env.client);
    store.getState().choose({ selected: ["a", "b"], anchor: "a", focus: "a" });
    store.getState().startNew(newRule());
    expect(store.getState().selection.selected).toEqual([]);
    store.getState().saved("n");
    expect(store.getState().selection).toEqual({ selected: ["n"], anchor: "n" });
    store.getState().forget();
    expect(store.getState().selection.selected).toEqual([]);
  });
});

/** The rule list (a listbox) and its options' selected state, by rule name. */
function listState(): Record<string, boolean> {
  const list = screen.getByRole("listbox", { name: "Rules, in the order they run" });
  return Object.fromEntries(
    within(list)
      .getAllByRole("option")
      .map((o) => [
        must(o.querySelector('[data-part="rules.list.name"]')).textContent,
        o.getAttribute("aria-selected") === "true",
      ]),
  );
}

describe("several rules in the rules window", () => {
  it("selects with Ctrl / Shift and the keys; the editor shows the focused rule", async () => {
    const env = setup();
    addTagAllRule(env);
    const { user, footer } = await openEditor(env, "Rust announcements");
    const list = screen.getByRole("listbox", { name: "Rules, in the order they run" });
    expect(list).toHaveAttribute("aria-multiselectable", "true");
    expect(within(footer).getByRole("button", { name: "Run" })).toBeInTheDocument();

    await user.keyboard("{Control>}");
    await user.click(screen.getByRole("option", { name: ruleOption("Tag all") }));
    await user.keyboard("{/Control}");
    expect(listState()).toEqual({ "Rust announcements": true, "Tag all": true });
    const editor = await screen.findByRole("form", { name: "Rule" });
    expect(await within(editor).findByDisplayValue("Tag all")).toBeInTheDocument();
    expect(
      within(footerOf(editor)).getByRole("button", { name: "Run 2 rules" }),
    ).toBeInTheDocument();

    // Escape goes back to the focused rule alone (and does not close anything).
    expect(screen.getByRole("option", { name: ruleOption("Tag all") })).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(listState()).toEqual({ "Rust announcements": false, "Tag all": true });
    // Shift+Up extends to the rule above and moves the focus there.
    await user.keyboard("{Shift>}{ArrowUp}{/Shift}");
    expect(listState()).toEqual({ "Rust announcements": true, "Tag all": true });
    await waitFor(() => {
      expect(screen.getByRole("option", { name: ruleOption("Rust announcements") })).toHaveFocus();
    });
    // Space removes the focused rule; Ctrl+A selects every rule again.
    await user.keyboard(" ");
    expect(listState()).toEqual({ "Rust announcements": false, "Tag all": true });
    await user.keyboard("{Control>}a{/Control}");
    expect(listState()).toEqual({ "Rust announcements": true, "Tag all": true });
    // A plain click selects only that rule.
    await user.click(screen.getByRole("option", { name: ruleOption("Tag all") }));
    expect(listState()).toEqual({ "Rust announcements": false, "Tag all": true });
    // Shift+click selects the range from the anchor.
    await user.keyboard("{Shift>}");
    await user.click(screen.getByRole("option", { name: ruleOption("Rust announcements") }));
    await user.keyboard("{/Shift}");
    expect(listState()).toEqual({ "Rust announcements": true, "Tag all": true });
  });

  it("selects rules that are not next to each other with the keyboard alone", async () => {
    const env = setup();
    addTagAllRule(env);
    addArchiveAllRule(env);
    const { user } = await openEditor(env, "Rust announcements");
    const first = screen.getByRole("option", { name: ruleOption("Rust announcements") });
    first.focus();
    // Ctrl+Down twice moves past the middle rule without selecting anything.
    await user.keyboard("{Control>}{ArrowDown}{ArrowDown}{/Control}");
    const last = screen.getByRole("option", { name: ruleOption("Archive all") });
    await waitFor(() => {
      expect(last).toHaveFocus();
    });
    expect(first).toHaveAttribute("aria-selected", "true");
    expect(last).toHaveAttribute("aria-selected", "false");
    // Space adds the focused rule: the first and the third are selected.
    await user.keyboard(" ");
    const list = screen.getByRole("listbox", { name: "Rules, in the order they run" });
    expect(
      within(list)
        .getAllByRole("option", { selected: true })
        .map((o) => o.getAttribute("data-rule")),
    ).toEqual(["rust-announcements", "archive-all"]);
    expect(screen.getByRole("option", { name: ruleOption("Tag all") })).toHaveAttribute(
      "aria-selected",
      "false",
    );
    // The hint names the keys.
    expect(screen.getByText(/Ctrl\+Up or Ctrl\+Down moves without selecting/)).toBeInTheDocument();
  });

  it("runs the selected rules in file order with one RunRules, and undoes them together", async () => {
    const env = setup(IN_INBOX);
    addTagAllRule(env);
    const { user, runOn } = await openEditor(env, "Tag all");
    await user.selectOptions(runOn, "folder");
    await user.keyboard("{Control>}");
    await user.click(screen.getByRole("option", { name: ruleOption("Rust announcements") }));
    await user.keyboard("{/Control}");
    const footer = footerOf(await screen.findByRole("form", { name: "Rule" }));
    await user.click(within(footer).getByRole("button", { name: "Run 2 rules" }));
    await within(footer).findByText(/^2 rules ran:/);
    expect(requestsOf(env, "RunRules")).toEqual([
      {
        type: "RunRules",
        rule_ids: ["rust-announcements", "tag-all"],
        message_ids: [],
        folder_id: INBOX,

        list: null,
      },
    ]);
    const tagged = () =>
      env.transport.data.messages.filter((m) => m.folder_id === INBOX && m.tags.includes("picked"))
        .length;
    expect(tagged()).toBeGreaterThan(0);
    await runsFinished(env);
    await user.click(within(footer).getByRole("button", { name: "Undo" }));
    await within(footer).findByText("Everything the run did was undone.");
    await waitFor(() => {
      expect(tagged()).toBe(0);
    });
  });

  it("tests each selected rule on its own and groups the preview by rule", async () => {
    const env = setup(IN_INBOX);
    addTagAllRule(env);
    const { user, runOn } = await openEditor(env, "Rust announcements");
    await user.selectOptions(runOn, "folder");
    await user.keyboard("{Shift>}");
    await user.click(screen.getByRole("option", { name: ruleOption("Tag all") }));
    await user.keyboard("{/Shift}");
    const editor = await screen.findByRole("form", { name: "Rule" });
    await user.click(within(footerOf(editor)).getByRole("button", { name: "Test rule" }));
    const preview = await screen.findByRole("region", { name: "Test results" });
    const inbox = env.transport.data.messages.filter((m) => m.folder_id === INBOX).length;
    expect(requestsOf(env, "TestRule").map((r) => r.rule.id)).toEqual([
      "rust-announcements",
      "tag-all",
    ]);
    const summary = within(preview).getAllByRole("status")[0];
    expect(summary).toHaveTextContent(/^2 rules tested: .* in all \(Current folder \(Inbox\)\)\.$/);
    expect(summary).toHaveFocus();
    expect(preview).toHaveTextContent("Each rule was tested on its own.");
    const groups = within(preview).getAllByRole("region");
    expect(groups.map((g) => g.getAttribute("aria-label"))).toEqual([
      "Rust announcements",
      "Tag all",
    ]);
    expect(within(must(groups[1])).getByRole("heading", { name: "Tag all" })).toBeInTheDocument();
    expect(
      within(must(groups[1])).getByRole("button", { name: `Apply to these ${String(inbox)}` }),
    ).toBeInTheDocument();
  });

  it("asks once before several rules run when one moves mail, with the total count", async () => {
    const env = setup(IN_INBOX);
    addArchiveAllRule(env);
    addTagAllRule(env);
    const inbox = env.transport.data.messages.filter((m) => m.folder_id === INBOX).length;
    const { user, runOn } = await openEditor(env, "Archive all");
    await user.selectOptions(runOn, "folder");
    await user.keyboard("{Control>}");
    await user.click(screen.getByRole("option", { name: ruleOption("Tag all") }));
    await user.keyboard("{/Control}");
    const footer = footerOf(await screen.findByRole("form", { name: "Rule" }));
    await user.click(within(footer).getByRole("button", { name: "Run 2 rules" }));
    const question = await findQuestion("Run rules");
    expect(question).toHaveTextContent(
      `Of these 2 rules, some move or delete mail. Together they have ${String(2 * inbox)} matches in Current folder (Inbox). Run them now?`,
    );
    expect(requestsOf(env, "RunRules")).toEqual([]);
    await user.click(within(question).getByRole("button", { name: "Run 2 rules" }));
    await within(footer).findByText(/^2 rules ran:/);
    expect(requestsOf(env, "RunRules").map((r) => r.rule_ids)).toEqual([
      ["archive-all", "tag-all"],
    ]);
  });

  it("names where a rule runs on Exchange accounts as the row's description", async () => {
    const env = exchangeSetup();
    renderWithProviders(<RuleManagerView />, env);
    const option = await screen.findByRole("option", { name: ruleOption("Rust announcements") });
    await waitFor(() => {
      expect(option).toHaveAccessibleDescription(/^Contoso: in Nail — .*Condition 2/);
    });
    // One badge per row, inside the option and not a control of its own.
    expect(option.querySelectorAll('[data-part="exchange.rule-badge"]')).toHaveLength(1);
    expect(within(option).queryByRole("button")).toBeNull();
  });

  it("hides the focused row's badge popover with Escape before anything else", async () => {
    const env = exchangeSetup();
    const closed: string[] = [];
    renderWithProviders(
      // eslint-disable-next-line jsx-a11y/no-static-element-interactions -- records an Escape the list lets through (the window would close).
      <div
        onKeyDown={(event) => {
          if (event.key === "Escape") closed.push("window");
        }}
      >
        <RuleManagerView />
      </div>,
      env,
    );
    const option = await screen.findByRole("option", { name: ruleOption("Rust announcements") });
    await waitFor(() => {
      expect(option.querySelector('[data-part="exchange.rule-badge"]')).not.toBeNull();
    });
    const user = userEvent.setup();
    await user.click(option);
    await user.keyboard("{Escape}");
    expect(option).toHaveAttribute("data-popover", "hidden");
    expect(closed).toEqual([]);
    // The next key shows it again; the next Escape does what Escape does.
    await user.keyboard("{Shift}");
    expect(option).not.toHaveAttribute("data-popover");
    await user.keyboard("{Escape}");
    expect(closed).toEqual([]);
    // Hidden already, with this rule alone selected: Escape goes on to the window.
    await user.keyboard("{Escape}");
    expect(closed).toEqual(["window"]);
  });

  it("describes a rule that is off", async () => {
    const env = setup();
    addTagAllRule(env, false);
    renderWithProviders(<RuleManagerView />, env);
    const option = await screen.findByRole("option", { name: ruleOption("Tag all") });
    expect(option).toHaveAccessibleDescription("Off");
    expect(option).toHaveAttribute("data-enabled", "false");
  });
});

describe("Values from this mail in a condition row", () => {
  it("sits right after the value field, before the case check box", async () => {
    const env = setup();
    const rule: Rule = {
      ...newRule(),
      id: "from-check",
      name: "From check",
      conditions: {
        type: "all",
        conditions: [
          {
            type: "address",
            field: "from",
            matches: { op: "contains", value: "", case_sensitive: false },
          },
        ],
      },
      actions: [{ type: "flag", flagged: true }],
    };
    env.transport.data.features.rules.rules.push(rule);
    const message = must(env.transport.data.messages.find((m) => m.from !== null));
    rulesStore(env.client)
      .getState()
      .setOrigin({ messageId: message.id, folderId: message.folder_id });
    const { editor } = await openEditor(env, "From check");
    const row = must(editor.querySelector<HTMLElement>('[data-part="rules.condition"]'));
    const values = await within(row).findByRole("button", { name: "Values from this mail" });
    const text = within(row).getByRole("combobox", { name: "Text" });
    const caseBox = within(row).getByRole("checkbox", { name: "Match upper and lower case" });
    const follows = (a: Element, b: Element) =>
      (a.compareDocumentPosition(b) & Node.DOCUMENT_POSITION_FOLLOWING) !== 0;
    expect(follows(text, values)).toBe(true);
    expect(follows(values, caseBox)).toBe(true);
  });
});
