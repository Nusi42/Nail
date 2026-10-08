import { expect, test, type Browser } from "@playwright/test";
import {
  expectWindowsInBackground,
  startDesktop,
  desktopTests,
  desktopPerFile,
  connectDesktop,
} from "../support/desktop";
import { freePort } from "../support/nail";
import {
  composerWindow,
  discardComposer,
  newMessageButton,
  openSettingsTab,
  showView,
  viewRegion,
  messageRows,
} from "../support/views";

/**
 * Smoke test of the desktop app (Tauri 2 + WebView2): the real
 * `nail-app.exe --seed demo`, driven by Playwright over the WebView2
 * DevTools protocol. It exercises the Tauri IPC path (`nail_call` and the
 * event channel) that the web tests do not.
 *
 * Needs Windows with the WebView2 runtime and `npm run build:desktop`. In
 * CI it runs only with `NAIL_E2E_DESKTOP=1`; locally it always runs.
 */

desktopTests(180_000);
// M4b: a fresh profile gets the first-run screen once (skipped).
const desktop = desktopPerFile();

test("the desktop app shows the demo mail through Tauri IPC", async () => {
  const page = desktop.page;
  // ADR 0146: the test app's window never reaches the developer's screen.
  await expectWindowsInBackground(page);
  const rows = messageRows(page);
  await expect(rows).toHaveCount(11, { timeout: 60_000 });
  await expect(page.locator('[data-part="folder-tree.item"]')).toHaveCount(3);
  // The built UI from the custom protocol, not a dev server.
  expect(page.url()).toMatch(/^(tauri:\/\/localhost|https?:\/\/tauri\.localhost)/);
  // Reading a message goes through `nail_call`.
  await rows.filter({ hasText: "Welcome to Nail" }).click();
  await expect(page.locator('[data-part="message-view.text"]')).toContainText("welcome aboard");
  // Search runs in the core (the seed is indexed).
  await page.keyboard.press("/");
  const search = page.getByRole("searchbox", { name: "Search mail" });
  await search.fill("nightingale");
  await search.press("Enter");
  await expect(rows.filter({ hasText: "Project Nightingale kickoff" }).first()).toBeVisible();
  await expect(rows.filter({ hasText: "Welcome to Nail" })).toHaveCount(0);
});

test("Shift+F10 opens the context menu of the selected message", async () => {
  const page = desktop.page;
  // The desktop's keyboard path (WebView2 could take Shift+F10 for its own
  // menu): Nail's menu for the focused row, closed with Escape.
  const row = messageRows(page).first();
  await row.click();
  await page.keyboard.press("Shift+F10");
  const menu = page.getByRole("menu");
  await expect(menu).toBeVisible();
  await expect(menu.getByRole("menuitem", { name: "Reply", exact: true })).toBeVisible();
  await page.keyboard.press("Escape");
  await expect(menu).toBeHidden();
});

test("the rich composer renders Markdown in the core and the MCP server runs", async () => {
  const page = desktop.page;
  await newMessageButton(page).click();
  const dialog = composerWindow(page);
  await expect(dialog).toBeVisible();
  await dialog.getByRole("radio", { name: "Markdown" }).click();
  await expect(dialog.getByRole("radio", { name: "Markdown" })).toBeChecked();
  await dialog.getByRole("textbox", { name: "Message" }).click();
  await page.keyboard.type("# Hello from the desktop\n\n*rendered by comrak*");
  const preview = dialog.frameLocator('[data-part="composer.preview.frame"]');
  await expect(preview.locator("h1")).toHaveText("Hello from the desktop");
  await discardComposer(page, dialog);
  await expect(dialog).toBeHidden();

  // The app starts the MCP server with the core as its data provider.
  await showView(page, "Settings");
  const settings = viewRegion(page, "Settings");
  await openSettingsTab(page, "MCP clients");
  await expect(settings.locator('[data-part="mcp-clients.server"]')).toHaveAttribute(
    "data-state",
    "running",
  );
  await expect(settings.locator('[data-part="mcp-clients.server"]')).toContainText(
    /http:\/\/127\.0\.0\.1:\d+\/mcp/,
  );
});

test("a mailto: link the app is started with opens the composer", async () => {
  // A second app of its own (seeded runs are independent instances).
  const linked = await startDesktop(freePort(), [
    "mailto:grace@navy.example?subject=Hello%20from%20a%20link&body=See%20you%20soon",
  ]);
  let browser: Browser | undefined;
  try {
    const connected = await connectDesktop(linked);
    browser = connected.browser;
    const window = connected.page;
    // The composer opened by the link comes first (no first-run screen over it).
    const dialog = composerWindow(window);
    await expect(dialog).toBeVisible({ timeout: 60_000 });
    await expect(dialog.getByRole("textbox", { name: "Subject" })).toHaveValue("Hello from a link");
    await expect(dialog.getByRole("textbox", { name: "To" })).toHaveValue(/grace@navy\.example/);
    await expect(dialog.getByRole("textbox", { name: "Message" })).toHaveValue(/^See you soon/);
    await expectWindowsInBackground(window);
  } finally {
    await browser?.close().catch(() => undefined);
    await linked.stop();
  }
});
