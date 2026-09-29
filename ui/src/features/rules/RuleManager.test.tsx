import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { ActionContext } from "../../actions/registry";
import type { MessageSummary, Rule } from "../../api/generated";
import { queryKeys } from "../../state/queries";
import { createEnv, renderNode, renderWithProviders, widget, type Env } from "../../test/harness";
import { appRegistry } from "../registry";
import { onRulesEvent } from "./api";
import { actions, dialogs, widgets } from "./index";
import { RuleManagerView } from "./RuleManager";
import { rulesStore } from "./store";

function setup(): Env {
  const env = createEnv();
  env.transport.data.behaviour.workMs = 0;
  // As the app shell does with the feature's event handlers.
  env.transport.onEvent((event) => onRulesEvent(event, env.queryClient));
  return env;
}

function field(container: HTMLElement, name: string): HTMLElement {
  const el = container.querySelector(`[name="${name}"]`);
  if (!(el instanceof HTMLElement)) throw new Error(`no field ${name}`);
  return el;
}

async function openSample(env: Env) {
  const user = userEvent.setup();
  const view = renderWithProviders(<RuleManagerView />, env);
  await user.click(await screen.findByRole("button", { name: "Rust announcements" }));
  const editor = await screen.findByRole("form", { name: "Rule" });
  return { user, view, editor };
}

describe("rule manager", () => {
  it("is registered as a widget, a dialog and actions", () => {
    expect(appRegistry.widgets["rule-manager"]).toBe(widgets["rule-manager"]);
    expect(appRegistry.dialogs.rules).toBe(dialogs.rules);
    expect(Object.keys(actions)).toEqual(["rules.open", "rules.create-from-message"]);
  });

  it("lists the rules and edits the selected one", async () => {
    const env = setup();
    renderNode(widget("rule-manager"), env);
    const list = await screen.findByRole("list", { name: "Rules, in the order they run" });
    expect(within(list).getByText("when mail arrives")).toBeInTheDocument();
    expect(screen.getByText("Choose a rule to edit it, or create a new one.")).toBeInTheDocument();
    const user = userEvent.setup();
    await user.click(screen.getByRole("button", { name: "Rust announcements" }));
    const editor = await screen.findByRole("form", { name: "Rule" });
    expect(within(editor).getByRole("textbox", { name: "Name" })).toHaveValue("Rust announcements");
    expect(within(editor).getByRole("checkbox", { name: "When mail arrives" })).toBeChecked();
    // Two conditions in an `any` group, two actions.
    expect(editor.querySelector('[data-part="rules.group"]')).toHaveAttribute("data-mode", "any");
    expect(editor.querySelectorAll('[data-part="rules.condition"]')).toHaveLength(2);
    expect(editor.querySelectorAll('[data-part="rules.action"]')).toHaveLength(2);

    await user.clear(within(editor).getByRole("textbox", { name: "Name" }));
    await user.type(within(editor).getByRole("textbox", { name: "Name" }), "Rust news");
    await user.click(within(editor).getByRole("button", { name: "Save rule" }));
    expect(await within(editor).findByText("The rule is saved.")).toBeInTheDocument();
    const saved = env.transport.requests.find((r) => r.type === "SaveRule");
    expect(saved).toMatchObject({ rule: { id: "rust-announcements", name: "Rust news" } });
    expect(await screen.findByRole("button", { name: "Rust news" })).toBeInTheDocument();
  });

  it("creates a rule with nested conditions and actions", async () => {
    const env = setup();
    const user = userEvent.setup();
    const { container } = renderWithProviders(<RuleManagerView />, env);
    await user.click(await screen.findByRole("button", { name: "New rule" }));
    const editor = await screen.findByRole("form", { name: "Rule" });
    const save = within(editor).getByRole("button", { name: "Save rule" });
    expect(save).toBeDisabled();
    expect(within(editor).getByText("Give the rule a name.")).toBeInTheDocument();
    expect(within(editor).getByText("Add at least one action.")).toBeInTheDocument();

    await user.type(within(editor).getByRole("textbox", { name: "Name" }), "Invoices");
    // A subject condition …
    await user.selectOptions(field(container, "add-"), "subject");
    await user.click(
      within(editor).getAllByRole("button", { name: "Add condition" })[0] as HTMLElement,
    );
    await user.selectOptions(field(container, "c-0-op"), "regex");
    await user.type(field(container, "c-0-value"), "invoice #\\d+");
    // … and a nested "any" group with an attachment condition.
    await user.selectOptions(field(container, "add-"), "any");
    await user.click(
      within(editor).getAllByRole("button", { name: "Add condition" })[0] as HTMLElement,
    );
    await user.selectOptions(field(container, "add-1"), "attachment");
    await user.click(
      within(editor).getAllByRole("button", { name: "Add condition" })[0] as HTMLElement,
    );
    expect(container.querySelectorAll('[data-part="rules.group"]')).toHaveLength(2);
    // Actions: add a tag, then move it below an archive.
    await user.selectOptions(field(container, "add-action"), "tag");
    await user.click(within(editor).getByRole("button", { name: "Add action" }));
    await user.type(field(container, "a-0-tag"), "invoice");
    await user.selectOptions(field(container, "add-action"), "archive");
    await user.click(within(editor).getByRole("button", { name: "Add action" }));
    await user.click(within(editor).getAllByRole("button", { name: "Up" })[1] as HTMLElement);

    expect(save).toBeEnabled();
    await user.click(save);
    await within(editor).findByText("The rule is saved.");
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
      await screen.findByRole("button", { name: "Invoices", pressed: true }),
    ).toBeInTheDocument();
    expect(rulesStore(env.client).getState().editing).toMatchObject({
      kind: "saved",
      id: "invoices",
    });
    // The editor stays, now for a saved rule.
    expect(within(editor).getByRole("button", { name: "Run now" })).toBeInTheDocument();
  });

  it("removes conditions and changes group modes", async () => {
    const env = setup();
    const { user, view, editor } = await openSample(env);
    await user.selectOptions(field(view.container, "c-mode"), "all");
    expect(editor.querySelector('[data-part="rules.group"]')).toHaveAttribute("data-mode", "all");
    await user.click(within(editor).getAllByRole("button", { name: "Remove" })[0] as HTMLElement);
    expect(editor.querySelectorAll('[data-part="rules.condition"]')).toHaveLength(1);
    expect(field(view.container, "c-0-kind")).toHaveValue("query");
  });

  it("edits the same rule as text and back", async () => {
    const env = setup();
    const { user, view, editor } = await openSample(env);
    await user.click(within(editor).getByRole("button", { name: "Text" }));
    const source = (await within(editor).findByRole("textbox", {
      name: "The rule in rules.kdl syntax",
    })) as HTMLTextAreaElement;
    expect(source.value).toContain('rule "Rust announcements" trigger="arrival" {');
    expect(source.value).toContain('tag "rust"');

    // An error is shown with its position; the view stays on the text.
    await user.clear(source);
    await user.type(source, 'rule "X" {{\n  then {{ explode }\n}');
    await user.click(within(editor).getByRole("button", { name: "Editor" }));
    expect(
      await within(editor).findByText(/Line 2, column \d+: unknown action `explode`/),
    ).toBeInTheDocument();

    await user.clear(source);
    await user.type(source, 'rule "Renamed" {{\n  then {{\n    tag "lang-rust"\n  }\n}');
    await user.click(within(editor).getByRole("button", { name: "Editor" }));
    await waitFor(() =>
      expect(within(editor).getByRole("textbox", { name: "Name" })).toHaveValue("Renamed"),
    );
    expect(field(view.container, "a-0-tag")).toHaveValue("lang-rust");
    // The rule keeps its id when saved from the text.
    await user.click(within(editor).getByRole("button", { name: "Save rule" }));
    await within(editor).findByText("The rule is saved.");
    expect(env.transport.requests.filter((r) => r.type === "SaveRule").pop()).toMatchObject({
      rule: { id: "rust-announcements", name: "Renamed" },
    });
  });

  it("tests a rule and explains the matches", async () => {
    const env = setup();
    const { user, editor } = await openSample(env);
    await user.click(within(editor).getByRole("button", { name: "Test rule" }));
    const results = await within(editor).findByRole("region", { name: "Test results" });
    expect(within(results).getByRole("status")).toHaveTextContent("2 messages of");
    const matches = results.querySelectorAll('[data-part="rules.test.match"]');
    expect(matches).toHaveLength(2);
    const first = matches[0] as HTMLElement;
    expect(within(first).getByText("[rust-announce] Rust 1.97 released")).toBeInTheDocument();
    expect(within(first).getByRole("list", { name: "What would happen" })).toHaveTextContent(
      "Add tag",
    );
    const why = within(first).getByRole("list", { name: "Why it matches" });
    expect(
      within(why).getByText("Subject matched: [rust-announce] Rust 1.97 released"),
    ).toBeInTheDocument();
    expect(within(why).getByText("Search query matched")).toBeInTheDocument();
    // Conditions that did not match are not reasons.
    expect(why.querySelectorAll('[data-part="rules.test.reason"]')).toHaveLength(2);
    expect(env.transport.requests.find((r) => r.type === "TestRule")).toMatchObject({
      folder_id: null,
      limit: 50,
    });
  });

  it("runs a rule and undoes the run from the history", async () => {
    const env = setup();
    const { user, editor } = await openSample(env);
    expect(await within(editor).findByText("This rule has not run yet.")).toBeInTheDocument();
    await user.click(within(editor).getByRole("button", { name: "Run now" }));
    expect(
      await within(editor).findByText("The rule is running on your mail."),
    ).toBeInTheDocument();
    const history = within(editor).getByRole("region", { name: "Recent runs" });
    const undo = await within(history).findByRole("button", { name: "Undo this run" });
    expect(within(history).getByText("By hand: 2 messages, done")).toBeInTheDocument();
    await user.click(undo);
    expect(await within(editor).findByText("The run was undone.")).toBeInTheDocument();
    expect(await within(history).findByText("By hand: 2 messages, undone")).toBeInTheDocument();
    expect(env.transport.data.messages.some((m) => m.tags.includes("rust"))).toBe(false);
  });

  it("reorders, disables and deletes rules", async () => {
    const env = setup();
    const extra: Rule = {
      ...env.transport.data.features.rules.rules[0],
      id: "second",
      name: "Second",
    } as Rule;
    env.transport.data.features.rules.rules.push(extra);
    const user = userEvent.setup();
    renderWithProviders(<RuleManagerView />, env);
    await user.click(await screen.findByRole("button", { name: "Move “Second” up" }));
    await waitFor(() =>
      expect(env.transport.data.features.rules.rules.map((r) => r.id)).toEqual([
        "second",
        "rust-announcements",
      ]),
    );
    expect(await screen.findByRole("button", { name: "Move “Second” up" })).toBeDisabled();

    await user.click(screen.getByRole("checkbox", { name: "Rule “Second” is on" }));
    await waitFor(() =>
      expect(env.transport.data.features.rules.rules[0]).toMatchObject({ enabled: false }),
    );

    await user.click(screen.getByRole("button", { name: "Second" }));
    const editor = await screen.findByRole("form", { name: "Rule" });
    await user.click(within(editor).getByRole("button", { name: "Delete rule" }));
    await waitFor(() =>
      expect(env.transport.data.features.rules.rules.map((r) => r.id)).toEqual([
        "rust-announcements",
      ]),
    );
    expect(
      await screen.findByText("Choose a rule to edit it, or create a new one."),
    ).toBeInTheDocument();
  });

  it("shows errors of the core", async () => {
    const env = setup();
    const { user, view, editor } = await openSample(env);
    await user.selectOptions(field(view.container, "c-0-op"), "regex");
    await user.clear(field(view.container, "c-0-value"));
    await user.type(field(view.container, "c-0-value"), "(unclosed");
    await user.click(within(editor).getByRole("button", { name: "Save rule" }));
    expect(await within(editor).findByRole("status")).toHaveAttribute("data-kind", "error");
  });
});

describe("create rule from message", () => {
  function ctxFor(env: Env): ActionContext {
    return {
      client: env.client,
      store: env.store,
      queryClient: env.queryClient,
      layout: env.transport.data.layout,
      t: (id) => id,
      locale: "en",
    };
  }

  it("opens the rules window with a rule pre-filled from the selected message", async () => {
    const env = setup();
    const ctx = ctxFor(env);
    const handler = actions["rules.create-from-message"];
    expect(handler?.enabled?.(ctx, [])).toBe(false);

    const message = env.transport.data.messages.find((m) =>
      m.subject.startsWith("[rust-announce]"),
    ) as MessageSummary;
    env.queryClient.setQueryData(queryKeys.messages(message.folder_id, 50), {
      pages: [{ offset: 0, total: 1, messages: [message] }],
      pageParams: [0],
    });
    env.store.getState().selectMessage(message.id);
    expect(handler?.enabled?.(ctx, [])).toBe(true);
    await handler?.run(ctx, []);
    expect(env.store.getState().windows).toContain("rules");

    const user = userEvent.setup();
    renderWithProviders(<RuleManagerView />, env);
    const editor = await screen.findByRole("form", { name: "Rule" });
    expect(editor).toHaveAttribute("data-new", "true");
    expect(within(editor).getByRole("textbox", { name: "Name" })).toHaveValue("Rust Announce");
    expect(editor.querySelectorAll('[data-part="rules.condition"]')).toHaveLength(2);
    // The user picks an action and saves.
    await user.click(within(editor).getByRole("button", { name: "Add action" }));
    await user.type(within(editor).getByRole("textbox", { name: "Folder path" }), "Lists");
    await user.click(within(editor).getByRole("button", { name: "Save rule" }));
    await within(editor).findByText("The rule is saved.");
    expect(env.transport.data.features.rules.rules.map((r) => r.id)).toContain("rust-announce");
  });

  it("opens the rules window", () => {
    const env = setup();
    void actions["rules.open"]?.run(ctxFor(env), []);
    expect(env.store.getState().windows).toEqual(["rules"]);
  });
});
