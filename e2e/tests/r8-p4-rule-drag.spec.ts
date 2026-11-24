import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test } from "@playwright/test";
import { nailPerTest } from "../support/nail";
import { openRulesWindow, openWorkInbox, ruleInList, rulesList } from "../support/rules";

/**
 * R8 (backlog "Rule list drag"): a rule is reordered by dragging its handle
 * in the rules window (R2, ADR 0222); the list and `rules.kdl` take the new
 * order, and a filtered list offers no handle to drag.
 */

const server = nailPerTest({ seed: "demo-multi" });

const NAMES = ["First rule", "Second rule", "Third rule"] as const;

/** Three manual rules in this order, written as the user's `rules.kdl`. */
const RULES = NAMES.map(
  (name) => `rule "${name}" trigger="manual" {\n    then {\n        flag\n    }\n}\n`,
).join("\n");

/** The rule names in `rules.kdl`, in file order. */
function namesInFile(): string[] {
  const text = readFileSync(path.join(server.configDir, "rules.kdl"), "utf8");
  return [...text.matchAll(/^rule "([^"]+)"/gm)].map((m) => m[1] ?? "");
}

test("dragging a rule by its handle reorders the list and rules.kdl", async ({ page }) => {
  // The core watches `rules.kdl` for hand edits.
  writeFileSync(path.join(server.configDir, "rules.kdl"), RULES);
  await openWorkInbox(page, server);
  const window = await openRulesWindow(page);
  const options = rulesList(window).getByRole("option");
  await expect(options).toHaveCount(NAMES.length);
  await expect(options.locator('[data-part="rules.list.name"]')).toHaveText([...NAMES]);

  // The third rule's handle onto the first rule: it runs first now.
  const handle = ruleInList(window, "Third rule").locator('[data-part="rules.list.handle"]');
  await handle.dragTo(ruleInList(window, "First rule"));
  const reordered = ["Third rule", "First rule", "Second rule"];
  await expect(options.locator('[data-part="rules.list.name"]')).toHaveText(reordered);
  await expect.poll(namesInFile).toEqual(reordered);
  // The moved rule keeps the focus.
  await expect(ruleInList(window, "Third rule")).toBeFocused();

  // Back down: the first row's handle onto the last row.
  await ruleInList(window, "Third rule")
    .locator('[data-part="rules.list.handle"]')
    .dragTo(ruleInList(window, "Second rule"));
  await expect(options.locator('[data-part="rules.list.name"]')).toHaveText([...NAMES]);
  await expect.poll(namesInFile).toEqual([...NAMES]);

  // A filtered list cannot be reordered: its handles are off.
  await window.getByRole("searchbox", { name: "Find a rule" }).fill("Second");
  await expect(
    ruleInList(window, "Second rule").locator('[data-part="rules.list.handle"]'),
  ).toHaveAttribute("data-disabled", "true");
  await expect(
    ruleInList(window, "Second rule").locator('[data-part="rules.list.handle"]'),
  ).toHaveAttribute("draggable", "false");
});
