/**
 * Test helpers of the rules feature (only tests import this file).
 */
import { waitFor, within } from "@testing-library/react";
import type { UserEvent } from "@testing-library/user-event";
import type { Rule } from "../../api/generated";
import { createEnv, type Env } from "../../test/harness";
import { onRulesEvent } from "./api";

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
