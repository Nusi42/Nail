import { existsSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect } from "@playwright/test";
import { nailFixture } from "../support/nail";
import { openSettings, toolbarAction, messageRows, messageList } from "../support/views";
import { defined } from "../support/defined";
import { readTextOrEmpty } from "../support/files";

/**
 * Customization tooling (ADR 0026) on a private `nail serve`: the inspector,
 * divider write-back into layout.kdl, the merge dialog with backup and
 * revert, and every error of every broken config file at once.
 */
const test = nailFixture();

const accent = () =>
  getComputedStyle(document.documentElement).getPropertyValue("--nail-color-accent").trim();

test("the inspector shows the part path, theme rules and layout node", async ({ page, nail }) => {
  await page.goto(nail.url);
  const row = messageRows(page).first();
  await expect(row).toBeVisible();
  await messageList(page).focus();
  await page.keyboard.press("Control+Shift+I");
  const panel = page.getByRole("complementary", { name: "Inspector" });
  await expect(panel.getByText("Nothing picked yet.")).toBeVisible();

  await row.locator('[data-part="message-list.row.subject"]').hover();
  const pathItems = panel.locator('[data-part="inspector.path.item"]');
  await expect(pathItems.filter({ hasText: /^message-list\.row\.subject$/ })).toHaveAttribute(
    "data-state",
    "current",
  );
  await expect(pathItems.filter({ hasText: /^message-list$/ })).toBeVisible();
  await expect(panel.getByText("message-list #list")).toBeVisible();
  // M5b: the panes live in the mail view's window.
  await expect(panel.getByText(/window “mail”, breakpoint 0, path 1\/4/)).toBeVisible();
  await expect(panel.getByText(/defaults\/layout\.kdl:\d+:\d+/)).toBeVisible();
  await expect(page.locator('[data-part="inspector.highlight"]')).toBeVisible();
  // List rows are styled by the UI skin since M5a; the pane divider still
  // has rules in the default theme.
  await page.locator('[data-part="split.divider"]').first().hover();
  await expect(panel.getByText(/defaults\/theme\.kdl:\d+:\d+/).first()).toBeVisible();
  await row.locator('[data-part="message-list.row.subject"]').hover();

  // A click pins the element; Esc leaves the inspector.
  await row.click();
  await expect(panel.getByText("Showing the element you clicked.")).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(panel).toBeHidden();
});

test("dragging a divider writes the sizes into layout.kdl", async ({ page, nail }) => {
  await page.goto(nail.url);
  await expect(messageRows(page).first()).toBeVisible();
  const layoutFile = path.join(nail.configDir, "layout.kdl");
  expect(existsSync(layoutFile)).toBe(false);

  const divider = page.getByRole("separator", { name: "Resize panes" }).first();
  const box = defined(await divider.boundingBox(), "the divider's box");
  const x = box.x + box.width / 2;
  const y = box.y + box.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + 60, y, { steps: 6 });
  await page.mouse.up();

  // Written once the dragging settles, as a sparse file over the defaults.
  await expect
    .poll(() => readTextOrEmpty(layoutFile), {
      timeout: 10_000,
    })
    .toMatch(/split "horizontal" id="panes"[\s\S]*sizes="\d+px 1fr 1\.4fr"/);
  const text = readFileSync(layoutFile, "utf8");
  expect(text).toContain('extends="default"');
  const width = Number(/sizes="(\d+)px/.exec(text)?.[1]);
  expect(width).toBeGreaterThan(280);
  // Still one pane layout after reloading from the file.
  await page.reload();
  await expect(
    page.locator('[data-part="split"][data-node-id="panes"] > [data-part="split.pane"]'),
  ).toHaveCount(3);
});

const SHARED_THEME = `// A shared theme
theme "Shared" extends="default" {
    tokens {
        color "accent" "#aa3300"
        color "unread" "#aa3300"
    }
}
`;

test("the merge dialog applies a shared theme and reverts it", async ({ page, nail }) => {
  writeFileSync(
    path.join(nail.configDir, "theme.kdl"),
    'theme "Mine" extends="default" {\n    tokens {\n        color "accent" "#115599"\n    }\n}\n',
  );
  await page.goto(nail.url);
  await expect.poll(() => page.evaluate(accent)).toBe("#115599");

  const settings = await openSettings(page, "Layout & toolbars");
  await toolbarAction(
    page,
    settings.getByRole("toolbar", { name: "Customization" }),
    "Apply a theme or layout…",
  );
  const dialog = page.getByRole("dialog", { name: "Apply a theme or layout" });
  await dialog.getByRole("textbox").fill(SHARED_THEME);
  await dialog.getByRole("button", { name: "Compare with mine" }).click();
  await expect(dialog.getByRole("heading", { name: "Tokens" })).toBeVisible();
  await dialog.getByRole("button", { name: "Take all theirs" }).click();
  await dialog.getByRole("button", { name: "Apply" }).click();
  await expect(dialog.getByText(/Applied\. Your previous file was backed up/)).toBeVisible();

  await expect.poll(() => page.evaluate(accent)).toBe("#aa3300");
  expect(readFileSync(path.join(nail.configDir, "theme.kdl"), "utf8")).toContain("#aa3300");
  expect(existsSync(path.join(nail.configDir, "backups"))).toBe(true);

  await dialog.getByRole("button", { name: "Revert" }).click();
  await expect(dialog.getByText("The previous version is back.")).toBeVisible();
  await expect.poll(() => page.evaluate(accent)).toBe("#115599");
});

test("the config error list shows every error of two broken files", async ({ page, nail }) => {
  await page.goto(nail.url);
  await expect(messageRows(page).first()).toBeVisible();
  // Nothing is shown while every file is valid.
  await expect(page.locator('[data-part="config-errors"][data-state="invalid"]')).toHaveCount(0);

  // Misspelled on purpose: two unknown properties. spellchecker:off
  writeFileSync(
    path.join(nail.configDir, "theme.kdl"),
    'theme "Broken" extends="default" {\n    part "app" {\n        colr "red"\n    }\n    part "toolbar" {\n        paddin 4\n    }\n}\n',
  );
  // spellchecker:on
  writeFileSync(
    path.join(nail.configDir, "layout.kdl"),
    'layout "Broken" extends="default" {\n    window "main" {\n        no-such-widget\n    }\n}\n',
  );
  const errors = page.locator('[data-part="config-errors"][data-state="invalid"]').first();
  await expect(errors.getByRole("alert")).toHaveText(/3 errors in 2 files/, { timeout: 15_000 });
  await expect(errors.getByRole("heading", { name: "theme.kdl — 2 errors" })).toBeVisible();
  await expect(errors.getByRole("heading", { name: "layout.kdl — 1 error" })).toBeVisible();
  await expect(errors.getByText(/theme\.kdl:3:\d+/)).toBeVisible();

  // Fixing one file removes its errors.
  writeFileSync(path.join(nail.configDir, "layout.kdl"), 'layout "Fine" extends="default" {\n}\n');
  await expect(errors.getByRole("alert")).toHaveText(/2 errors in 1 file/);
  await expect(errors.getByRole("heading", { name: "layout.kdl — 1 error" })).toHaveCount(0);
});
