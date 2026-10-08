import { expect, test, type Browser, type Locator, type Page } from "@playwright/test";
import {
  expectWindowsInBackground,
  shellControl,
  desktopTests,
  configWithIdentity,
  desktopPerTest,
} from "../support/desktop";
import {
  composerWindow,
  composerWindows,
  messageRows,
  newMessageButton,
  runFromPalette,
  windowById,
  windowSelector,
} from "../support/views";
import { defined } from "../support/defined";

/**
 * M5c on the real desktop app (ADRs 0119, 0121): a composer pops out into
 * its own OS window (a second WebView2 on the same core), is written and
 * sent there, and its OS window closes; a second composer comes back to
 * the main window with "Return to main window" and keeps what was typed;
 * a popped-out composer shows its questions in its own window (Discard,
 * and the OS close button with unsaved typing: ADR 0122).
 *
 * Needs Windows with the WebView2 runtime and `npm run build:desktop`. In
 * CI it runs only with `NAIL_E2E_DESKTOP=1`; locally it always runs.
 */

desktopTests();
test.use({ actionTimeout: 30_000 });

const launchApp = desktopPerTest();
/** The connection of the test's app (its pop-out windows are pages of it). */
let browser: Browser | undefined;

async function launch(): Promise<Page> {
  const session = await launchApp({ args: ["--config-dir", configWithIdentity()] });
  browser = session.browser;
  const page = session.page;
  await expect(messageRows(page).first()).toBeVisible({
    timeout: 60_000,
  });
  return page;
}

interface ShellPopOut {
  label: string;
  title: string;
  visible: boolean;
}

const popOuts = (page: Page) => shellControl(page, "pop-outs") as Promise<ShellPopOut[]>;

/** The pop-out page of `window` (its URL ends with `#/window/<id>`). */
async function popOutPage(window: string): Promise<Page> {
  let found: Page | undefined;
  await expect
    .poll(
      () => {
        found = browser
          ?.contexts()
          .flatMap((c) => c.pages())
          .find((p) => p.url().endsWith(`#/window/${window}`));
        return found !== undefined;
      },
      { timeout: 60_000 },
    )
    .toBe(true);
  return defined(found, "the pop-out page");
}

/**
 * Clicks a button that closes its own pop-out page: the OS window may be
 * gone before Playwright's click returns, which is the expected outcome.
 */
async function clickClosing(button: Locator, popOut: Page): Promise<void> {
  try {
    await button.click();
  } catch (error) {
    if (!popOut.isClosed()) throw error;
  }
}

/** {@link clickClosing}, then waits until no pop-out is left. */
async function clickClosingLast(button: Locator, popOut: Page, main: Page): Promise<void> {
  await clickClosing(button, popOut);
  await expect.poll(async () => (await popOuts(main)).length, { timeout: 30_000 }).toBe(0);
}

async function newComposer(page: Page, subject: string): Promise<string> {
  const before = await composerWindows(page).count();
  await newMessageButton(page).click();
  await expect(composerWindows(page)).toHaveCount(before + 1);
  const composer = composerWindow(page);
  // The identity of configWithIdentity (the demo seed has none).
  await composer.getByRole("combobox", { name: "From" }).selectOption("ada@example.org");
  await composer.getByRole("textbox", { name: "To" }).fill("grace@navy.example");
  await composer.getByRole("textbox", { name: "Subject" }).fill(subject);
  return defined(await composer.getAttribute("data-window"), "the composer's window id");
}

test("a composer pops out, is sent from its own window, and the window closes", async () => {
  const page = await launch();

  const id = await newComposer(page, "From its own window");
  const inPage = windowById(page, id);
  await inPage.getByRole("button", { name: "Open in new window" }).click();
  // It left the page and has an OS window titled by its subject.
  await expect(inPage).toHaveCount(0);
  await expect
    .poll(async () => (await popOuts(page)).map((p) => [p.label, p.visible]), { timeout: 30_000 })
    .toEqual([[`popout-${id}`, true]]);
  await expect
    .poll(async () => (await popOuts(page))[0]?.title, { timeout: 30_000 })
    .toBe("From its own window");
  // Shown, yet cloaked and never activated (ADR 0146).
  const shown = await expectWindowsInBackground(page);
  expect(shown.windows.map((w) => w.label)).toEqual(["main", `popout-${id}`]);

  // The pop-out page shows the composer with the draft it was saved as.
  const popped = await popOutPage(id);
  const host = popped.locator(`[data-part="pop-out"][data-window="${id}"]`);
  await expect(host).toBeVisible();
  await expect(host.getByRole("textbox", { name: "Subject" })).toHaveValue("From its own window");
  await expect(host.getByRole("textbox", { name: "To" })).toHaveValue(/grace@navy\.example/);
  await host.getByRole("textbox", { name: "Message" }).fill("Typed in the pop-out.");
  await clickClosing(host.getByRole("button", { name: "Send", exact: true }), popped);

  // Sent: the OS window closes; the message is in the main window's
  // outbox (this profile's SMTP server is unreachable, so it stays queued).
  await expect.poll(async () => (await popOuts(page)).length, { timeout: 30_000 }).toBe(0);
  await runFromPalette(page, "outbox", "Outbox");
  await expect(
    page.locator('[data-part="outbox.item"]').filter({ hasText: "From its own window" }),
  ).toBeVisible({ timeout: 30_000 });
  await expect(composerWindows(page)).toHaveCount(0);
});

test("a popped-out composer returns to the main window with what was typed", async () => {
  const page = await launch();

  const id = await newComposer(page, "Round trip");
  const inPage = windowById(page, id);
  await inPage.getByRole("button", { name: "Open in new window" }).click();
  await expect(inPage).toHaveCount(0);
  const popped = await popOutPage(id);
  const host = popped.locator(`[data-part="pop-out"][data-window="${id}"]`);
  await expect(host.getByRole("textbox", { name: "Subject" })).toHaveValue("Round trip");
  await host.getByRole("textbox", { name: "Message" }).fill("Written outside.");
  // The OS window closes and the composer is back in the page, with the text.
  await clickClosingLast(host.getByRole("button", { name: "Return to main window" }), popped, page);
  await expect(inPage).toBeVisible({ timeout: 30_000 });
  await expect(inPage.getByRole("textbox", { name: "Subject" })).toHaveValue("Round trip");
  await expect(inPage.getByRole("textbox", { name: "Message" })).toHaveValue("Written outside.");
});

test("a popped-out composer asks its questions in its own window (Discard, OS close)", async () => {
  const page = await launch();

  // Discard: the question shows in the pop-out page, and discarding closes
  // the OS window.
  const first = await newComposer(page, "Throw away");
  const firstInPage = windowById(page, first);
  await firstInPage.getByRole("button", { name: "Open in new window" }).click();
  await expect(firstInPage).toHaveCount(0);
  const thrown = await popOutPage(first);
  const thrownHost = thrown.locator(`[data-part="pop-out"][data-window="${first}"]`);
  await expect(thrownHost.getByRole("textbox", { name: "Subject" })).toHaveValue("Throw away");
  await thrownHost.getByRole("button", { name: "Discard", exact: true }).click();
  const discard = thrown.locator(windowSelector(`composer-discard:${first.split(":")[1] ?? ""}`));
  await expect(discard).toBeVisible();
  await clickClosingLast(
    discard.getByRole("button", { name: "Discard", exact: true }),
    thrown,
    page,
  );

  // The OS close button (Alt+F4) with unsaved typing: the page asks first,
  // and "Save draft" keeps the text and closes the OS window.
  const second = await newComposer(page, "Keep this");
  const secondInPage = windowById(page, second);
  await secondInPage.getByRole("button", { name: "Open in new window" }).click();
  await expect(secondInPage).toHaveCount(0);
  const kept = await popOutPage(second);
  const keptHost = kept.locator(`[data-part="pop-out"][data-window="${second}"]`);
  await expect(keptHost.getByRole("textbox", { name: "Subject" })).toHaveValue("Keep this");
  await keptHost.getByRole("textbox", { name: "Message" }).fill("The last words.");
  await shellControl(page, "close-pop-outs");
  const question = kept.locator(windowSelector(`composer-close:${second.split(":")[1] ?? ""}`));
  await expect(question).toBeVisible();
  // Still open while it asks.
  expect((await popOuts(page)).map((p) => p.label)).toEqual([`popout-${second}`]);
  await clickClosingLast(question.getByRole("button", { name: "Save draft" }), kept, page);

  // The draft has the last words: continued from the outbox's drafts (it
  // opens in the page or, as the kind was last popped out, in a pop-out).
  await runFromPalette(page, "outbox", "Outbox");
  const draft = page
    .locator('[data-part="outbox.item"][data-state="draft"]')
    .filter({ hasText: "Keep this" });
  await expect(draft).toBeVisible({ timeout: 30_000 });
  await draft.locator('[data-action="continue-draft"]').click();
  await expect
    .poll(
      async () => {
        const pages = browser?.contexts().flatMap((c) => c.pages()) ?? [];
        for (const p of pages) {
          const found = await p
            .evaluate(() =>
              [...document.querySelectorAll("textarea")].some((t) => t.value === "The last words."),
            )
            .catch(() => false);
          if (found) return true;
        }
        return false;
      },
      { timeout: 60_000 },
    )
    .toBe(true);
});

/** The page's size (the window's inner size in CSS pixels). */
async function pageSize(page: Page) {
  return page.evaluate(() => ({ width: window.innerWidth, height: window.innerHeight }));
}

test("windows follow the OS window's size; WebView2 shows no menu or reload of its own", async () => {
  const page = await launch();
  // ADR 0184: WebView2's own context menus are off in every window.
  const menus = () =>
    shellControl(page, "default-context-menus") as Promise<Record<string, boolean>>;
  await expect.poll(async () => (await menus()).main).toBe(false);

  // ADR 0181: the composer follows the main window, smaller and larger.
  const id = await newComposer(page, "Resized");
  const inPage = page.locator(`[data-part="window"][data-window="${id}"]`);
  const rect = async () => defined(await inPage.boundingBox(), "the composer's box");
  for (const [width, height] of [
    [900, 560],
    [1400, 1000],
  ] as const) {
    await shellControl(page, `resize-main:${String(width)}x${String(height)}`);
    // DPI scaling may round the inner size by a pixel.
    await expect
      .poll(async () => Math.abs((await pageSize(page)).width - width))
      .toBeLessThanOrEqual(2);
    const size = await pageSize(page);
    await expect
      .poll(async () => {
        const box = await rect();
        return (
          box.x >= 0 &&
          box.y >= 0 &&
          box.x + box.width <= size.width + 1 &&
          box.y + box.height <= size.height + 1
        );
      })
      .toBe(true);
    const box = await rect();
    expect(Math.round(box.width)).toBe(Math.min(820, size.width));
    expect(Math.round(box.height)).toBe(Math.min(720, size.height));
  }

  // ADR 0183: F5 / Ctrl+R in a field check for mail; the page is not reloaded.
  await page.evaluate(() => Reflect.set(window, "nailStillHere", true));
  await inPage.getByRole("textbox", { name: "Subject" }).focus();
  await page.keyboard.press("F5");
  await page.keyboard.press("Control+R");
  await expect(inPage.getByRole("textbox", { name: "Subject" })).toHaveValue("Resized");
  expect(await page.evaluate(() => Reflect.get(window, "nailStillHere") === true)).toBe(true);

  // The popped-out composer fills its OS window at every size.
  await inPage.getByRole("button", { name: "Open in new window" }).click();
  const popped = await popOutPage(id);
  const host = popped.locator(`[data-part="pop-out"][data-window="${id}"]`);
  await expect(host).toBeVisible();
  for (const [width, height] of [
    [640, 480],
    [1000, 760],
  ] as const) {
    await shellControl(page, `resize-pop-outs:${String(width)}x${String(height)}`);
    await expect
      .poll(async () => Math.abs((await pageSize(popped)).width - width))
      .toBeLessThanOrEqual(2);
    const size = await pageSize(popped);
    await expect
      .poll(async () => {
        const box = defined(await host.boundingBox(), "the pop-out page");
        return [Math.round(box.width), Math.round(box.height)];
      })
      .toEqual([size.width, size.height]);
    await expect(host.getByRole("button", { name: "Send", exact: true })).toBeInViewport();
  }
  await expect.poll(async () => (await menus())[`popout-${id}`]).toBe(false);
  await clickClosingLast(host.getByRole("button", { name: "Return to main window" }), popped, page);
});
