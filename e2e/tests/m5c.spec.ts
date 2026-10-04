import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { startNail, type NailServer } from "../support/nail";
import { composerWindow, composerWindows, runFromPalette } from "../support/views";

/**
 * Wave M5c (ADRs 0116–0121) against `nail serve --seed demo`: the window
 * manager — move by the title bar, resize from each edge and corner, snap
 * to the left / right half, maximize by double-click, keyboard move /
 * resize (arrow keys on the title bar, Alt+F7 / Alt+F8), geometry
 * remembered across a reload (`window-state.json`) — two composers side by
 * side, modal confirmations that block only their own flow, and Escape.
 * Each test runs its own server: remembered geometry is per config dir.
 * The desktop pop-out is in `desktop-m5c.spec.ts`.
 */

let server: NailServer;

test.afterEach(async () => {
  await server?.stop();
});

const rows = (page: Page) => page.locator('[data-part="message-list.row"]');
const win = (page: Page, id: string) => page.locator(`[data-part="window"][data-window="${id}"]`);
const titleBar = (window: Locator) => window.locator('[data-part="window.header"]');

async function open(page: Page, seed: "demo" | "demo-multi" = "demo") {
  server = await startNail({ seed });
  await page.goto(server.url);
  await expect(rows(page).first()).toBeVisible();
}

async function openRules(page: Page): Promise<Locator> {
  await runFromPalette(page, "filter rules", "Filter rules");
  const rules = win(page, "rules");
  await expect(rules).toBeVisible();
  return rules;
}

async function box(locator: Locator) {
  const b = await locator.boundingBox();
  if (b === null) throw new Error("not visible");
  return {
    x: Math.round(b.x),
    y: Math.round(b.y),
    width: Math.round(b.width),
    height: Math.round(b.height),
  };
}

/** Drags `from` (its centre, or an offset inside it) by dx / dy with the mouse. */
async function dragBy(
  page: Page,
  from: Locator,
  dx: number,
  dy: number,
  options: { up?: boolean; at?: { x: number; y: number } } = {},
) {
  const b = await box(from);
  const x = options.at ? b.x + options.at.x : b.x + b.width / 2;
  const y = options.at ? b.y + options.at.y : b.y + b.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move(x + dx / 2, y + dy / 2, { steps: 4 });
  await page.mouse.move(x + dx, y + dy, { steps: 4 });
  if (options.up !== false) await page.mouse.up();
}

/** Drags the title bar (at its left part, away from the buttons) to a page point. */
async function dragTitleTo(page: Page, window: Locator, to: { x: number; y: number }, up = true) {
  const bar = await box(titleBar(window));
  const x = bar.x + 60;
  const y = bar.y + bar.height / 2;
  await page.mouse.move(x, y);
  await page.mouse.down();
  await page.mouse.move((x + to.x) / 2, (y + to.y) / 2, { steps: 4 });
  await page.mouse.move(to.x, to.y, { steps: 6 });
  if (up) await page.mouse.up();
}

const near = (actual: number, expected: number, tolerance = 2) =>
  expect(Math.abs(actual - expected), `${actual} ≈ ${expected}`).toBeLessThanOrEqual(tolerance);

test("a window moves by its title bar and resizes from every edge and corner", async ({ page }) => {
  await open(page);
  const rules = await openRules(page);
  // Non-modal: no scrim, the main window stays usable behind it.
  await expect(rules).not.toHaveAttribute("aria-modal", "true");
  await expect(page.locator('[data-part="window-scrim"]')).toHaveCount(0);
  const start = await box(rules);
  expect(start).toMatchObject({ width: 900, height: 640 });

  await dragBy(page, titleBar(rules), 60, 40, { at: { x: 60, y: 20 } });
  const moved = await box(rules);
  near(moved.x, start.x + 60);
  near(moved.y, start.y + 40);
  expect(moved).toMatchObject({ width: 900, height: 640 });

  // Each handle: outward by 30 px grows that side, the opposite edges stay.
  const cases: [string, number, number, Partial<Record<"x" | "y" | "width" | "height", number>>][] =
    [
      ["e", 30, 0, { width: 30 }],
      ["s", 0, 30, { height: 30 }],
      ["w", -30, 0, { x: -30, width: 30 }],
      ["n", 0, -30, { y: -30, height: 30 }],
      ["se", 30, 30, { width: 30, height: 30 }],
      ["nw", -30, -30, { x: -30, y: -30, width: 30, height: 30 }],
      ["ne", 30, -30, { y: -30, width: 30, height: 30 }],
      ["sw", -30, 30, { x: -30, width: 30, height: 30 }],
    ];
  for (const [edge, dx, dy, change] of cases) {
    const before = await box(rules);
    await dragBy(page, rules.locator(`[data-part="window.handle"][data-edge="${edge}"]`), dx, dy);
    const after = await box(rules);
    for (const key of ["x", "y", "width", "height"] as const) {
      near(after[key], before[key] + (change[key] ?? 0));
    }
  }

  // Never below the minimum size (600 × 420).
  await dragBy(page, rules.locator('[data-part="window.handle"][data-edge="se"]'), -900, -900);
  expect(await box(rules)).toMatchObject({ width: 600, height: 420 });
});

test("windows snap to the left and right half and come loose again", async ({ page }) => {
  await open(page);
  const rules = await openRules(page);
  const viewport = page.viewportSize()!;
  const before = await box(rules);

  await dragTitleTo(page, rules, { x: 5, y: 300 }, false);
  await expect(page.locator('[data-part="window-snap-preview"][data-snap="left"]')).toBeVisible();
  await page.mouse.up();
  await expect(rules).toHaveAttribute("data-snap", "left");
  await expect(rules).toHaveAttribute("data-state", /snapped/);
  await expect(page.locator('[data-part="window-snap-preview"]')).toHaveCount(0);
  let snapped = await box(rules);
  near(snapped.x, 0);
  near(snapped.width, viewport.width / 2, 2);
  near(snapped.height, viewport.height, 2);

  await dragTitleTo(page, rules, { x: viewport.width - 4, y: 300 });
  await expect(rules).toHaveAttribute("data-snap", "right");
  snapped = await box(rules);
  near(snapped.x + snapped.width, viewport.width);
  near(snapped.width, viewport.width / 2, 2);

  // Dragged away from the edge, it comes loose at its old size.
  await dragTitleTo(page, rules, { x: 640, y: 200 });
  await expect(rules).not.toHaveAttribute("data-snap", /.+/);
  expect(await box(rules)).toMatchObject({ width: before.width, height: before.height });
});

test("a double-click on the title bar maximizes and restores", async ({ page }) => {
  await open(page);
  const rules = await openRules(page);
  const before = await box(rules);
  const viewport = page.viewportSize()!;

  await titleBar(rules).locator('[data-part="window.title"]').dblclick();
  await expect(rules).toHaveAttribute("data-state", /maximized/);
  expect(await box(rules)).toMatchObject({
    x: 0,
    y: 0,
    width: viewport.width,
    height: viewport.height,
  });
  await expect(rules.getByRole("button", { name: "Restore" })).toBeVisible();

  await titleBar(rules).locator('[data-part="window.title"]').dblclick();
  await expect(rules).not.toHaveAttribute("data-state", /maximized/);
  expect(await box(rules)).toEqual(before);
  // The button does the same.
  await rules.getByRole("button", { name: "Maximize" }).click();
  await expect(rules).toHaveAttribute("data-state", /maximized/);
  await rules.getByRole("button", { name: "Restore" }).click();
  expect(await box(rules)).toEqual(before);
});

test("the keyboard moves and resizes a window (arrows, Alt+F7, Alt+F8)", async ({ page }) => {
  await open(page);
  const rules = await openRules(page);
  const bar = titleBar(rules);
  await expect(bar).toHaveAccessibleName("Title bar of Filter rules");
  await bar.focus();
  const start = await box(rules);

  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("ArrowDown");
  let now = await box(rules);
  near(now.x, start.x + 16, 1);
  near(now.y, start.y + 16, 1);
  await page.keyboard.press("Shift+ArrowLeft");
  await page.keyboard.press("Shift+ArrowUp");
  now = await box(rules);
  expect(now).toMatchObject({ width: start.width - 16, height: start.height - 16 });

  // Alt+F7: move mode; Escape goes back and keeps the window open.
  const beforeMode = await box(rules);
  await page.keyboard.press("Alt+F7");
  await expect(rules).toHaveAttribute("data-mode", "move");
  await page.keyboard.press("ArrowLeft");
  await page.keyboard.press("ArrowLeft");
  near((await box(rules)).x, beforeMode.x - 32, 1);
  await page.keyboard.press("Escape");
  await expect(rules).not.toHaveAttribute("data-mode", /.+/);
  await expect(rules).toBeVisible();
  expect(await box(rules)).toEqual(beforeMode);

  // Alt+F8: size mode; Enter keeps the result.
  await page.keyboard.press("Alt+F8");
  await expect(rules).toHaveAttribute("data-mode", "size");
  // The live region announces the mode for screen readers.
  await expect(rules.locator('[data-part="window.status"]')).not.toBeEmpty();
  await page.keyboard.press("ArrowRight");
  await page.keyboard.press("Enter");
  await expect(rules).not.toHaveAttribute("data-mode", /.+/);
  expect(await box(rules)).toMatchObject({ width: beforeMode.width + 16 });
});

test("window geometry is remembered across a reload", async ({ page }) => {
  await open(page);
  let rules = await openRules(page);
  await dragBy(page, titleBar(rules), -100, -60, { at: { x: 60, y: 20 } });
  await dragBy(page, rules.locator('[data-part="window.handle"][data-edge="se"]'), -120, -80);
  const placed = await box(rules);
  expect(placed).toMatchObject({ width: 780, height: 560 });

  // Saved per kind in <config-dir>/window-state.json shortly after the change.
  const file = path.join(server.configDir, "window-state.json");
  await expect
    .poll(() => {
      if (!existsSync(file)) return null;
      const saved = JSON.parse(readFileSync(file, "utf8")) as {
        windows: Record<string, { page?: { rect: { width: number; height: number } } }>;
      };
      return saved.windows.rules?.page?.rect ?? null;
    })
    .toMatchObject({ width: 780, height: 560 });

  await page.reload();
  await expect(rows(page).first()).toBeVisible();
  rules = await openRules(page);
  expect(await box(rules)).toEqual(placed);

  // "Reset window positions" forgets it: back to the default, centred.
  await runFromPalette(page, "reset window positions", "Reset window positions");
  await expect.poll(async () => (await box(rules)).width).toBe(900);
});

test("two composers side by side: each edits and sends on its own", async ({ page }) => {
  // demo-multi: accounts with identities and a mock SMTP server.
  await open(page, "demo-multi");
  const viewport = page.viewportSize()!;
  await rows(page).first().click();
  await page.keyboard.press("c");
  const first = composerWindow(page);
  await expect(first).toBeVisible();
  const firstId = (await first.getAttribute("data-window"))!;
  expect(firstId).toMatch(/^composer:/);
  const left = win(page, firstId);
  await left.getByRole("textbox", { name: "Subject" }).fill("Left draft");
  // The title bar shows the subject.
  await expect(left.locator('[data-part="window.title"]')).toHaveText("Left draft");
  await dragTitleTo(page, left, { x: 5, y: 400 });
  await expect(left).toHaveAttribute("data-snap", "left");

  // Back to the main window, a second message.
  await page.keyboard.press("Control+Shift+F6");
  await expect(page.locator('[data-part="window"]:focus-within')).toHaveCount(0);
  await page.keyboard.press("c");
  await expect(composerWindows(page)).toHaveCount(2);
  const secondId = (await composerWindow(page).getAttribute("data-window"))!;
  expect(secondId).not.toBe(firstId);
  const right = win(page, secondId);
  await dragTitleTo(page, right, { x: viewport.width - 4, y: 400 });
  await expect(right).toHaveAttribute("data-snap", "right");
  const [a, b] = [await box(left), await box(right)];
  expect(a.x + a.width).toBeLessThanOrEqual(b.x + 2);

  await right.getByRole("textbox", { name: "To" }).fill("grace@navy.example");
  await right.getByRole("textbox", { name: "Subject" }).fill("Right one");
  await left.getByRole("textbox", { name: "Message" }).fill("Still writing the left one.");
  await right.getByRole("textbox", { name: "Message" }).fill("Sent from the right.");
  await right.getByRole("button", { name: "Send", exact: true }).click();
  await expect(right).toBeHidden();
  await expect(page.getByRole("status", { name: "Status" }).getByText("Message sent.")).toBeVisible(
    { timeout: 20_000 },
  );
  await expect(left).toBeVisible();
  await expect(left.getByRole("textbox", { name: "Subject" })).toHaveValue("Left draft");
  await expect(left.getByRole("textbox", { name: "Message" })).toHaveValue(
    "Still writing the left one.",
  );
});

test("a modal confirmation blocks only its own flow", async ({ page }) => {
  await open(page);
  await rows(page).first().click();
  await page.keyboard.press("c");
  const composer = composerWindow(page);
  const id = (await composer.getAttribute("data-window"))!;
  const own = win(page, id);
  await own.getByRole("textbox", { name: "Subject" }).fill("Unsaved thoughts");
  // Closed with unsaved changes (before the autosave): the composer asks.
  await own.locator('[data-part="window.close"]').click();
  const key = id.split(":")[1];
  const confirm = win(page, `composer-close:${key}`);
  await expect(confirm).toBeVisible();
  await expect(confirm).toHaveAttribute("data-owner", id);
  await expect(own).toHaveAttribute("data-blocked", "");
  await expect(own).toHaveAttribute("inert", "");
  await expect(page.locator('[data-part="window-scrim"]')).toHaveCount(0);

  // The main window stays usable: another message opens in the reader.
  const other = rows(page).nth(2);
  await other.click();
  await expect(other).toHaveAttribute("aria-selected", "true");

  await confirm.getByRole("button", { name: "Keep editing" }).click();
  await expect(confirm).toBeHidden();
  await expect(own).not.toHaveAttribute("inert", "");
  await expect(own.getByRole("textbox", { name: "Subject" })).toHaveValue("Unsaved thoughts");

  // A confirmation from the main window ("Delete permanently?") covers the
  // main window, not the composer (snapped right, out of the list's way).
  await dragTitleTo(page, own, { x: page.viewportSize()!.width - 4, y: 400 });
  await expect(own).toHaveAttribute("data-snap", "right");
  await rows(page)
    .first()
    .click({ position: { x: 40, y: 20 } });
  await page.keyboard.press("Shift+Delete");
  const remove = win(page, "mail-delete-permanently");
  await expect(remove).toBeVisible();
  await expect(remove).toHaveAttribute("aria-modal", "true");
  await expect(remove).toHaveAttribute("data-owner", "main");
  await expect(page.locator('[data-part="window-scrim"]')).toBeVisible();
  await own.getByRole("textbox", { name: "Message" }).fill("Still usable.");
  await expect(own.getByRole("textbox", { name: "Message" })).toHaveValue("Still usable.");
  await remove.getByRole("button", { name: "Cancel" }).click();
  await expect(remove).toBeHidden();
  await expect(page.locator('[data-part="window-scrim"]')).toHaveCount(0);
});

test("Escape closes the focused window, not windows behind the main window", async ({ page }) => {
  await open(page);
  const rules = await openRules(page);
  // The new window has the focus.
  await expect(rules.locator(":focus")).toHaveCount(1);
  await page.keyboard.press("Escape");
  await expect(rules).toBeHidden();

  // Focus in the main window: Escape there leaves a non-modal window open.
  await openRules(page);
  await rows(page).first().click();
  await page.keyboard.press("Escape");
  await expect(win(page, "rules")).toBeVisible();

  // A composer with unsaved changes asks instead of closing; Escape in the
  // confirmation closes only the confirmation.
  await page.keyboard.press("c");
  const composer = composerWindow(page);
  const id = (await composer.getAttribute("data-window"))!;
  const own = win(page, id);
  await own.getByRole("textbox", { name: "Subject" }).fill("Not yet");
  await own.getByRole("textbox", { name: "Subject" }).press("Escape");
  const confirm = win(page, `composer-close:${id.split(":")[1]}`);
  await expect(confirm).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(confirm).toBeHidden();
  await expect(own).toBeVisible();
});
