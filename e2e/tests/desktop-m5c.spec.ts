import { expect, test, type Browser, type Page } from "@playwright/test";
import {
  connectDesktop,
  shellControl,
  skipFirstRun,
  startDesktop,
  type DesktopApp,
} from "../support/desktop";
import { writeFileSync } from "node:fs";
import path from "node:path";
import { freePort, tempDir } from "../support/nail";
import {
  composerWindow,
  composerWindows,
  newMessageButton,
  runFromPalette,
} from "../support/views";

/**
 * M5c on the real desktop app (ADRs 0119, 0121): a composer pops out into
 * its own OS window (a second WebView2 on the same core), is written and
 * sent there, and its OS window closes; a second composer comes back to
 * the main window with "Return to main window" and keeps what was typed.
 *
 * Needs Windows with the WebView2 runtime and `npm run build:desktop`. In
 * CI it runs only with `NAIL_E2E_DESKTOP=1`; locally it always runs.
 */

test.skip(process.platform !== "win32", "the desktop app uses WebView2 (Windows)");
test.skip(
  !!process.env.CI && !process.env.NAIL_E2E_DESKTOP,
  "set NAIL_E2E_DESKTOP=1 to run the desktop smoke tests in CI",
);
test.describe.configure({ mode: "serial", timeout: 240_000 });
test.use({ actionTimeout: 30_000 });

let app: DesktopApp | undefined;
let browser: Browser | undefined;

test.afterEach(async () => {
  await browser?.close().catch(() => undefined);
  await app?.stop();
  app = undefined;
  browser = undefined;
});

/** An accounts.kdl with one identity (switched off: nothing connects). */
function configWithIdentity(): string {
  const dir = tempDir("desktop-config");
  writeFileSync(
    path.join(dir, "accounts.kdl"),
    [
      'account "work" email="ada@example.org" name="Work" enabled=#false {',
      '    imap host="127.0.0.1" port=1 tls="plaintext"',
      '    smtp host="127.0.0.1" port=1 tls="plaintext"',
      '    identity "ada@example.org" name="Ada Lovelace"',
      "}",
      "",
    ].join("\n"),
  );
  return dir;
}

async function launch(): Promise<Page> {
  app = await startDesktop(freePort(), { args: ["--config-dir", configWithIdentity()] });
  const connected = await connectDesktop(app);
  browser = connected.browser;
  const page = connected.page;
  await skipFirstRun(page);
  await expect(page.locator('[data-part="message-list.row"]').first()).toBeVisible({
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
  return found!;
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
  return (await composer.getAttribute("data-window"))!;
}

test("a composer pops out, is sent from its own window, and the window closes", async () => {
  const page = await launch();

  const id = await newComposer(page, "From its own window");
  const inPage = page.locator(`[data-part="window"][data-window="${id}"]`);
  await inPage.getByRole("button", { name: "Open in new window" }).click();
  // It left the page and has an OS window titled by its subject.
  await expect(inPage).toHaveCount(0);
  await expect
    .poll(async () => (await popOuts(page)).map((p) => [p.label, p.visible]), { timeout: 30_000 })
    .toEqual([[`popout-${id}`, true]]);
  await expect
    .poll(async () => (await popOuts(page))[0]?.title, { timeout: 30_000 })
    .toBe("From its own window");

  // The pop-out page shows the composer with the draft it was saved as.
  const popped = await popOutPage(id);
  const host = popped.locator(`[data-part="pop-out"][data-window="${id}"]`);
  await expect(host).toBeVisible();
  await expect(host.getByRole("textbox", { name: "Subject" })).toHaveValue("From its own window");
  await expect(host.getByRole("textbox", { name: "To" })).toHaveValue(/grace@navy\.example/);
  await host.getByRole("textbox", { name: "Message" }).fill("Typed in the pop-out.");
  await host.getByRole("button", { name: "Send", exact: true }).click();

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
  const inPage = page.locator(`[data-part="window"][data-window="${id}"]`);
  await inPage.getByRole("button", { name: "Open in new window" }).click();
  await expect(inPage).toHaveCount(0);
  const popped = await popOutPage(id);
  const host = popped.locator(`[data-part="pop-out"][data-window="${id}"]`);
  await expect(host.getByRole("textbox", { name: "Subject" })).toHaveValue("Round trip");
  await host.getByRole("textbox", { name: "Message" }).fill("Written outside.");
  await host.getByRole("button", { name: "Return to main window" }).click();

  // The OS window closes and the composer is back in the page, with the text.
  await expect.poll(async () => (await popOuts(page)).length, { timeout: 30_000 }).toBe(0);
  await expect(inPage).toBeVisible({ timeout: 30_000 });
  await expect(inPage.getByRole("textbox", { name: "Subject" })).toHaveValue("Round trip");
  await expect(inPage.getByRole("textbox", { name: "Message" })).toHaveValue("Written outside.");
});
