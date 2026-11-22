/**
 * Test helpers of the rules feature (only tests import this file).
 */
import { screen, waitFor, within } from "@testing-library/react";
import userEvent, { type UserEvent } from "@testing-library/user-event";
import { createElement } from "react";
import type { Rule } from "../../api/generated";
import { WindowLayer } from "../../layout/WindowLayer";
import { withCloseGuard } from "../../test/closeGuard";
import { createEnv, renderWithProviders, type Env } from "../../test/harness";
import { INBOX } from "../../test/mockIds";
import { onRulesEvent } from "./api";
import { RULES_WINDOW } from "./windows";

/** A test app whose mock core answers at once and whose rules queries follow its events. */
export function setup(state: Parameters<typeof createEnv>[0] = {}): Env {
  const env = createEnv(state);
  env.transport.data.behaviour.workMs = 0;
  // As the app shell does with the feature's event handlers.
  env.transport.onEvent((event) => {
    onRulesEvent(event, env.queryClient);
  });
  return env;
}

/** A second rule that tags every message (`enabled` or off). */
export function addTagAllRule(env: Env, enabled = true): Rule {
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

/** A manual rule that moves every message of the Inbox to Archive. */
export function addArchiveAllRule(env: Env): Rule {
  const rule: Rule = {
    id: "archive-all",
    name: "Archive all",
    enabled: true,
    triggers: [{ type: "manual" }],
    scope: [],
    conditions: { type: "all", conditions: [] },
    actions: [{ type: "archive" }],
    source: null,
  };
  env.transport.data.features.rules.rules.push(rule);
  return rule;
}

/** The app state of a test whose main window lists the personal Inbox. */
export const IN_INBOX = { state: { activeAccountId: "acc-personal", activeFolderId: INBOX } };

/** Every run the mock recorded is finished (undo needs that). */
export async function runsFinished(env: Env): Promise<void> {
  await waitFor(() => {
    expect(env.transport.data.features.rules.runs.every((r) => r.state === "done")).toBe(true);
  });
}

/** Switches the rule editor to its text view; returns the text field. */
export async function showTextView(
  user: UserEvent,
  editor: HTMLElement,
): Promise<HTMLTextAreaElement> {
  await user.click(within(editor).getByRole("radio", { name: "Text" }));
  return within(editor).findByRole<HTMLTextAreaElement>("textbox", {
    name: "The rule in rules.kdl syntax",
  });
}

/**
 * Picks `item` from the menu of the `index`-th "+ Add …" button named
 * `button` inside `scope` (the condition and action menus).
 */
export async function addFromMenu(
  user: UserEvent,
  scope: HTMLElement,
  button: string,
  item: string,
  index = 0,
): Promise<void> {
  const buttons = within(scope).getAllByRole("button", { name: button });
  const opener = buttons[index];
  if (!opener) throw new Error(`no button ${button} #${String(index)}`);
  await user.click(opener);
  await user.click(within(scope).getByRole("menuitem", { name: item }));
}

/** The editor's footer, where Run on, Run, Test, Save and the status are. */
export function footerOf(editor: HTMLElement): HTMLElement {
  const footer = editor.querySelector<HTMLElement>('[data-part="rules.footer"]');
  if (!footer) throw new Error("no footer");
  return footer;
}

/**
 * The accessible name of the rule list's option of the rule `name`: the
 * name, then its triggers ("Rust announcements when mail arrives").
 */
export function ruleOption(name: string): RegExp {
  return new RegExp(`^${name.replace(/[.*+?^${}()|[\]\\]/g, "\\$&")} `);
}

/**
 * Renders the rules window in the window layer, with the close guard
 * installed as the app does (its questions open as windows over it).
 */
export function renderRulesWindow(env: Env) {
  withCloseGuard(env);
  env.store.setState({ windows: [RULES_WINDOW] });
  return renderWithProviders(
    createElement(WindowLayer, { layout: env.transport.data.layout }),
    env,
  );
}

/**
 * Renders the rules window (in the window layer, with the close guard
 * installed as the app does) and opens the rule named `name` from the
 * list, or a new rule (`null`); returns the editor, its footer and "Run on".
 */
export async function openEditor(env: Env, name: string | null) {
  const user = userEvent.setup({ delay: null });
  renderRulesWindow(env);
  await user.click(
    await (name === null
      ? screen.findByRole("button", { name: "New rule" })
      : screen.findByRole("option", { name: ruleOption(name) })),
  );
  const editor = await screen.findByRole("form", { name: "Rule" });
  const footer = footerOf(editor);
  return { user, editor, footer, runOn: within(footer).getByRole("combobox", { name: "Run on" }) };
}

/** The open question window titled `title` ("Delete rule", "Run rules", "Unsaved changes"). */
export function findQuestion(title: string): Promise<HTMLElement> {
  return screen.findByRole("dialog", { name: title });
}
