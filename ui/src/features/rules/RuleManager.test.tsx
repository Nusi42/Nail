import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Rule } from "../../api/generated";
import { queryKeys } from "../../state/queries";
import { renderNode, renderWithProviders, widget, type Env } from "../../test/harness";
import { actionContext } from "../../test/env";
import { must } from "../../test/must";
import { appRegistry } from "../registry";
import { actions, dialogs, widgets } from "./index";
import { RuleManagerView } from "./RuleManager";
import { rulesStore } from "./store";
import { addFromMenu, footerOf, ruleOption, setup, showTextView } from "./testing";

/** A field by its `name`, or the control inside the field row `data-field` (pickers). */
function field(container: HTMLElement, name: string): HTMLElement {
  const el =
    container.querySelector(`[name="${name}"]`) ??
    container.querySelector(`[data-field="${name}"] :is(input, select, textarea)`);
  if (!(el instanceof HTMLElement)) throw new Error(`no field ${name}`);
  return el;
}

async function openSample(env: Env) {
  const user = userEvent.setup({ delay: null });
  const view = renderWithProviders(<RuleManagerView />, env);
  await user.click(await screen.findByRole("option", { name: ruleOption("Rust announcements") }));
  const editor = await screen.findByRole("form", { name: "Rule" });
  return { user, view, editor };
}

describe("rule manager", () => {
  it("is registered as a widget, a dialog and actions", () => {
    expect(appRegistry.widgets["rule-manager"]).toBe(widgets["rule-manager"]);
    expect(appRegistry.dialogs.rules).toBe(dialogs.rules);
    expect(Object.keys(actions)).toEqual([
      "rules.open",
      "rules.create-from-message",
      "rules.run-menu",
      "rules.run",
      "rules.add-to-rule",
      "rules.add-to-rule.pick",
    ]);
  });

  it("lists the rules and edits the selected one", async () => {
    const env = setup();
    renderNode(widget("rule-manager"), env);
    const list = await screen.findByRole("listbox", { name: "Rules, in the order they run" });
    expect(list).toHaveAttribute("aria-multiselectable", "true");
    expect(within(list).getByText("when mail arrives")).toBeInTheDocument();
    expect(screen.getByText("Choose a rule to edit it, or create a new one.")).toBeInTheDocument();
    const user = userEvent.setup({ delay: null });
    await user.click(screen.getByRole("option", { name: ruleOption("Rust announcements") }));
    const editor = await screen.findByRole("form", { name: "Rule" });
    expect(within(editor).getByRole("textbox", { name: "Name" })).toHaveValue("Rust announcements");
    expect(within(editor).getByRole("switch", { name: "On" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(within(editor).getByRole("checkbox", { name: "When mail arrives" })).toBeChecked();
    // The root `any` group is the IF row's Match choice, not a second box.
    expect(field(editor, "c-mode")).toHaveValue("any");
    expect(editor.querySelector('[data-part="rules.group"]')).toBeNull();
    expect(editor.querySelectorAll('[data-part="rules.condition"]')).toHaveLength(2);
    expect(editor.querySelectorAll('[data-part="rules.action"]')).toHaveLength(2);
    // WHEN / WHERE / IF / THEN.
    expect(
      [...editor.querySelectorAll('[data-part="rules.form.section"]')].map((s) =>
        s.getAttribute("data-section"),
      ),
    ).toEqual(["when", "where", "if", "then"]);

    await user.clear(within(editor).getByRole("textbox", { name: "Name" }));
    await user.type(within(editor).getByRole("textbox", { name: "Name" }), "Rust news");
    await user.click(within(editor).getByRole("button", { name: "Save rule" }));
    expect(await within(editor).findByText("Saved")).toBeInTheDocument();
    const saved = env.transport.requests.find((r) => r.type === "SaveRule");
    expect(saved).toMatchObject({ rule: { id: "rust-announcements", name: "Rust news" } });
    expect(
      await screen.findByRole("option", { name: ruleOption("Rust news") }),
    ).toBeInTheDocument();
    // "Saved" goes with the next edit.
    await user.type(within(editor).getByRole("textbox", { name: "Name" }), "!");
    expect(within(editor).queryByText("Saved")).toBeNull();
  });

  it("creates a rule with nested conditions and actions", async () => {
    const env = setup();
    const user = userEvent.setup({ delay: null });
    const { container } = renderWithProviders(<RuleManagerView />, env);
    await user.click(await screen.findByRole("button", { name: "New rule" }));
    const editor = await screen.findByRole("form", { name: "Rule" });
    const save = within(editor).getByRole("button", { name: "Save rule" });
    // What is missing: a badge in the footer that Save names.
    expect(save).toHaveAttribute("aria-disabled", "true");
    const badge = within(footerOf(editor)).getByRole("button", { name: "2 things to fix" });
    expect(badge).toHaveAccessibleDescription(/Give the rule a name\.\s*Add at least one action\./);
    expect(save.getAttribute("aria-describedby")).toBe(badge.getAttribute("aria-describedby"));
    expect(within(editor).getByRole("textbox", { name: "Name" })).toHaveAttribute(
      "aria-invalid",
      "true",
    );

    await user.type(within(editor).getByRole("textbox", { name: "Name" }), "Invoices");
    // "+ Add condition" shows its name as text (no hover-only label).
    const add = within(editor).getAllByRole("button", { name: "Add condition" })[0];
    expect(add).toHaveTextContent("Add condition");
    expect(add).not.toHaveAttribute("title");
    // A subject condition …
    await addFromMenu(user, editor, "Add condition", "Subject");
    await user.selectOptions(field(container, "c-0-op"), "regex");
    // An empty value is marked until it is filled in.
    expect(field(container, "c-0-value")).toHaveAttribute("aria-invalid", "true");
    await user.type(field(container, "c-0-value"), "invoice #\\d+");
    expect(field(container, "c-0-value")).not.toHaveAttribute("aria-invalid");
    // … and a nested "any" group with an attachment condition.
    await addFromMenu(user, editor, "Add condition", "Any of a group");
    await addFromMenu(user, editor, "Add condition", "Attachment", 0);
    expect(container.querySelectorAll('[data-part="rules.group"]')).toHaveLength(1);
    // Actions: add a tag, then move it below an archive.
    await addFromMenu(user, editor, "Add action", "Add tag");
    await user.type(field(container, "a-0-tag"), "invoice");
    await addFromMenu(user, editor, "Add action", "Archive");
    await user.click(must(within(editor).getAllByRole("button", { name: "Up" })[1]));

    expect(save).not.toHaveAttribute("aria-disabled");
    await user.click(save);
    await within(editor).findByText("Saved");
    const request = env.transport.requests.find((r) => r.type === "SaveRule");
    expect(request).toMatchObject({
      position: null,
      rule: {
        id: "",
        name: "Invoices",
        triggers: [{ type: "arrival" }],
        conditions: {
          type: "all",
          conditions: [
            { type: "subject", matches: { op: "regex", value: "invoice #\\d+" } },
            { type: "any", conditions: [{ type: "attachment", name: null, mime: null }] },
          ],
        },
        actions: [{ type: "archive" }, { type: "tag", tag: "invoice" }],
      },
    });
    // The saved rule is selected, with its id.
    expect(
      await screen.findByRole("option", { name: ruleOption("Invoices"), selected: true }),
    ).toBeInTheDocument();
    expect(rulesStore(env.client).getState().editing).toMatchObject({
      kind: "saved",
      id: "invoices",
    });
    // The editor stays, now for a saved rule, and the rule can run at once.
    const footer = footerOf(editor);
    expect(within(footer).getByRole("button", { name: "Delete rule…" })).toBeInTheDocument();
    expect(within(footer).getByRole("button", { name: "Run" })).toBeEnabled();
  });

  it("removes conditions and changes group modes", async () => {
    const env = setup();
    const { user, view, editor } = await openSample(env);
    await user.selectOptions(field(view.container, "c-mode"), "all");
    expect(field(view.container, "c-mode")).toHaveValue("all");
    await user.click(must(within(editor).getAllByRole("button", { name: "Remove" })[0]));
    expect(editor.querySelectorAll('[data-part="rules.condition"]')).toHaveLength(1);
    expect(field(view.container, "c-0-kind")).toHaveValue("query");
  });

  it("edits the same rule as text and back", async () => {
    const env = setup();
    const { user, view, editor } = await openSample(env);
    const source = await showTextView(user, editor);
    expect(source.value).toContain('rule "Rust announcements" trigger="arrival" {');
    expect(source.value).toContain('tag "rust"');
    // Highlighted, with line numbers.
    const highlight = must(editor.querySelector('[data-part="rules.text.highlight"]'));
    expect(highlight.textContent).toBe(`${source.value}\n`);
    expect(highlight.querySelector('[data-kind="node"]')).toHaveTextContent("rule");
    expect(editor.querySelectorAll('[data-part="rules.text.line"]')).toHaveLength(
      source.value.split("\n").length,
    );

    // Text that does not read stays text; the footer badge says why, the
    // line is marked and the text field names the badge.
    await user.clear(source);
    await user.type(source, 'rule "X" {{\n  then {{ explode }\n}');
    await user.click(within(editor).getByRole("radio", { name: "Editor" }));
    const badge = await within(footerOf(editor)).findByRole("button", { name: "One error" });
    expect(badge.getAttribute("title")).toMatch(/Line 2, column \d+: unknown action `explode`/);
    expect(
      must(editor.querySelector<HTMLElement>('[data-part="rules.text.stuck"]')),
    ).toHaveTextContent(/The text has mistakes/);
    expect(source).toHaveAttribute("aria-invalid", "true");
    expect(
      editor.querySelector('[data-part="rules.text.line"][data-state="error"]'),
    ).toHaveTextContent("2");

    await user.clear(source);
    await user.type(source, 'rule "Renamed" {{\n  then {{\n    tag "lang-rust"\n  }\n}');
    await user.click(within(editor).getByRole("radio", { name: "Editor" }));
    await waitFor(() =>
      expect(within(editor).getByRole("textbox", { name: "Name" })).toHaveValue("Renamed"),
    );
    expect(field(view.container, "a-0-tag")).toHaveValue("lang-rust");
    // The rule keeps its id when saved from the text.
    await user.click(within(editor).getByRole("button", { name: "Save rule" }));
    await within(editor).findByText("Saved");
    expect(env.transport.requests.filter((r) => r.type === "SaveRule").pop()).toMatchObject({
      rule: { id: "rust-announcements", name: "Renamed" },
    });
  });

  it("discards text that does not read and goes back to the rule", async () => {
    const env = setup();
    const { user, editor } = await openSample(env);
    const source = await showTextView(user, editor);
    await user.clear(source);
    await user.type(source, "rule {{");
    await user.click(within(editor).getByRole("radio", { name: "Editor" }));
    await user.click(await within(editor).findByRole("button", { name: "Discard text changes" }));
    expect(within(editor).getByRole("textbox", { name: "Name" })).toHaveValue("Rust announcements");
    expect(within(editor).getByRole("radio", { name: "Editor" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
  });

  it("reorders, disables and deletes rules (asking first)", async () => {
    const env = setup();
    const extra: Rule = {
      ...env.transport.data.features.rules.rules[0],
      id: "second",
      name: "Second",
    } as Rule;
    env.transport.data.features.rules.rules.push(extra);
    const user = userEvent.setup({ delay: null });
    renderWithProviders(<RuleManagerView />, env);
    // Alt+Up on the focused row moves it up.
    await user.click(await screen.findByRole("option", { name: ruleOption("Second") }));
    await user.keyboard("{Alt>}{ArrowUp}{/Alt}");
    await waitFor(() => {
      expect(env.transport.data.features.rules.rules.map((r) => r.id)).toEqual([
        "second",
        "rust-announcements",
      ]);
    });
    // Alt+Down moves it back; the row keeps the focus.
    await waitFor(() => {
      expect(screen.getByRole("option", { name: ruleOption("Second") })).toHaveFocus();
    });
    await user.keyboard("{Alt>}{ArrowDown}{/Alt}");
    await waitFor(() => {
      expect(env.transport.data.features.rules.rules.map((r) => r.id)).toEqual([
        "rust-announcements",
        "second",
      ]);
    });

    // The On mark (a pointer target inside the option) switches it off.
    const on = screen
      .getByRole("option", { name: ruleOption("Second") })
      .querySelector<HTMLElement>('[data-part="rules.list.enabled"]');
    expect(on).toHaveAttribute("title", "Switch “Second” on or off");
    if (on) await user.click(on);
    await waitFor(() => {
      expect(env.transport.data.features.rules.rules[1]).toMatchObject({ enabled: false });
    });

    const editor = await screen.findByRole("form", { name: "Rule" });
    await user.click(within(editor).getByRole("button", { name: "Delete rule…" }));
    const question = await screen.findByRole("alertdialog");
    expect(question).toHaveTextContent("Delete the rule “Second”?");
    // The safe choice has the focus; the list cannot be used meanwhile.
    expect(within(question).getByRole("button", { name: "Cancel" })).toHaveFocus();
    expect(
      screen
        .getByRole("listbox", { name: "Rules, in the order they run", hidden: true })
        .closest("[inert]"),
    ).not.toBeNull();
    await user.click(within(question).getByRole("button", { name: "Cancel" }));
    expect(env.transport.data.features.rules.rules).toHaveLength(2);
    // The focus goes back to the button that asked.
    await waitFor(() => {
      expect(within(editor).getByRole("button", { name: "Delete rule…" })).toHaveFocus();
    });

    await user.click(within(editor).getByRole("button", { name: "Delete rule…" }));
    await user.click(
      within(await screen.findByRole("alertdialog")).getByRole("button", {
        name: "Delete the rule",
      }),
    );
    await waitFor(() => {
      expect(env.transport.data.features.rules.rules.map((r) => r.id)).toEqual([
        "rust-announcements",
      ]);
    });
    expect(
      await screen.findByText("Choose a rule to edit it, or create a new one."),
    ).toBeInTheDocument();
    // The button that asked is gone with the rule: the focus stays in the window.
    await waitFor(() => {
      expect(document.activeElement?.closest('[data-part="rules"]')).not.toBeNull();
    });
  });

  it("shows errors of the core in the footer's badge", async () => {
    const env = setup();
    const { user, view, editor } = await openSample(env);
    await user.selectOptions(field(view.container, "c-0-op"), "regex");
    await user.clear(field(view.container, "c-0-value"));
    await user.type(field(view.container, "c-0-value"), "(unclosed");
    await user.click(within(editor).getByRole("button", { name: "Save rule" }));
    const alert = await within(footerOf(editor)).findByRole("alert");
    expect(alert).toHaveAttribute("data-kind", "error");
    expect(within(alert).getByRole("button", { name: "One error" })).toHaveAttribute("title");
  });
});

describe("create rule from message", () => {
  it("opens the rules window with a rule pre-filled from the selected message", async () => {
    const env = setup();
    const ctx = actionContext(env);
    const handler = actions["rules.create-from-message"];
    expect(handler?.enabled?.(ctx, [])).toBe(false);

    const message = must(
      env.transport.data.messages.find((m) => m.subject.startsWith("[rust-announce]")),
    );
    env.queryClient.setQueryData(queryKeys.messages(message.folder_id, 50), {
      pages: [{ offset: 0, total: 1, messages: [message] }],
      pageParams: [0],
    });
    env.store.getState().selectMessage(message.id);
    expect(handler?.enabled?.(ctx, [])).toBe(true);
    await handler?.run(ctx, []);
    expect(env.store.getState().windows).toContain("rules");

    const user = userEvent.setup({ delay: null });
    renderWithProviders(<RuleManagerView />, env);
    const editor = await screen.findByRole("form", { name: "Rule" });
    expect(editor).toHaveAttribute("data-new", "true");
    expect(within(editor).getByRole("textbox", { name: "Name" })).toHaveValue("Rust Announce");
    expect(editor.querySelectorAll('[data-part="rules.condition"]')).toHaveLength(2);
    // The user picks an action and saves.
    await addFromMenu(user, editor, "Add action", "Move to folder");
    await user.type(within(editor).getByRole("combobox", { name: "Folder path" }), "Lists");
    await user.click(within(editor).getByRole("button", { name: "Save rule" }));
    await within(editor).findByText("Saved");
    expect(env.transport.data.features.rules.rules.map((r) => r.id)).toContain("rust-announce");
  });

  it("opens the rules window", () => {
    const env = setup();
    void actions["rules.open"]?.run(actionContext(env), []);
    expect(env.store.getState().windows).toEqual(["rules"]);
  });
});
