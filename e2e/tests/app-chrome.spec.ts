import { existsSync, readFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { control, startNail, type NailServer } from "../support/nail";
import {
  runFromPalette,
  accountSectionNamed,
  composerWindow,
  newMessageButton,
  openApp,
  READER_SUBJECT,
  rail,
  sectionInbox,
  showView,
  viewRegion,
  type ViewName,
} from "../support/views";
import { defined } from "../support/defined";
import { UI_LOG_TARGET as PAGE_LOG_TARGET } from "../../ui/src/api/generated/constants";

/**
 * The app's chrome as reviewed in the desktop app (2026-10-08):
 *
 * - windows follow the app's size (ADR 0181): a window the user has not
 *   placed is placed again for the new size and grows back to its own
 *   size; a maximized one fills the page; panes reflow;
 * - only the user's data is selectable, never Nail's labels (ADR 0182);
 * - Check for new mail beside New message, F5 / Ctrl+R (ADR 0183);
 * - a right-click opens Nail's menu for the place or nothing, never the
 *   browser's; text gets Cut / Copy / Paste / Select all (ADR 0184);
 * - Ctrl+Shift+R and "Reload window" reload the page, which reports its
 *   start to the core's app.log (ADR 0207).
 *
 * Each test has its own `nail serve` (windows remember their geometry).
 */

let server: NailServer;

test.afterEach(async () => {
  await server.stop();
});

const rows = (page: Page) => page.locator('[data-part="message-list.row"]');

/** A window's rectangle in the page. */
async function rectOf(locator: Locator) {
  const box = defined(await locator.boundingBox(), "the window's box");
  return {
    x: Math.round(box.x),
    y: Math.round(box.y),
    width: Math.round(box.width),
    height: Math.round(box.height),
  };
}

/** Whether `rect` lies wholly on a page of `width` × `height`. */
function onPage(rect: { x: number; y: number; width: number; height: number }, size: Size) {
  return (
    rect.x >= 0 &&
    rect.y >= 0 &&
    rect.x + rect.width <= size.width &&
    rect.y + rect.height <= size.height
  );
}

interface Size {
  width: number;
  height: number;
}

test.describe("windows follow the app's size (ADR 0181)", () => {
  test.beforeEach(async () => {
    server = await startNail({ seed: "demo" });
  });

  test("a composer fits the page, grows back, and a maximized one fills it", async ({ page }) => {
    // Opened on a short page: fitted to it.
    await page.setViewportSize({ width: 1000, height: 600 });
    await openApp(page, server.url);
    await newMessageButton(page).click();
    const composer = composerWindow(page);
    await expect(composer).toBeVisible();
    const small = await rectOf(composer);
    expect(small.height).toBe(600);
    expect(onPage(small, { width: 1000, height: 600 })).toBe(true);

    // The page grows: its own size (820 × 720, layout.kdl), centred.
    await page.setViewportSize({ width: 1280, height: 1000 });
    await expect.poll(() => rectOf(composer)).toEqual({ x: 230, y: 140, width: 820, height: 720 });

    // Smaller than the composer, at two sizes: inside the page, never cut off.
    for (const size of [
      { width: 900, height: 650 },
      { width: 700, height: 500 },
    ]) {
      await page.setViewportSize(size);
      await expect.poll(async () => onPage(await rectOf(composer), size)).toBe(true);
      const shown = await rectOf(composer);
      expect(shown.width).toBe(Math.min(820, size.width));
      expect(shown.height).toBe(Math.min(720, size.height));
      // The title bar and its buttons stay reachable.
      await expect(composer.getByRole("button", { name: "Close" })).toBeInViewport();
    }

    // Maximized: the whole page, whatever its size.
    await page.setViewportSize({ width: 1280, height: 1000 });
    await composer.getByRole("button", { name: "Maximize" }).click();
    for (const size of [
      { width: 1280, height: 1000 },
      { width: 900, height: 650 },
      { width: 1600, height: 1100 },
    ]) {
      await page.setViewportSize(size);
      await expect.poll(() => rectOf(composer)).toEqual({ x: 0, y: 0, ...size });
    }
  });

  test("a window the user placed keeps its size and place, inside the page", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 1000 });
    await openApp(page, server.url);
    await newMessageButton(page).click();
    const composer = composerWindow(page);
    const header = composer.locator('[data-part="window.header"]');
    const start = await rectOf(composer);
    // Dragged 100 px to the left and 60 px up by its title bar.
    await header.hover({ position: { x: 200, y: 10 } });
    await page.mouse.down();
    await page.mouse.move(start.x + 100, start.y - 50, { steps: 5 });
    await page.mouse.up();
    const placed = await rectOf(composer);
    expect(Math.abs(placed.x - (start.x - 100))).toBeLessThanOrEqual(1);
    expect(Math.abs(placed.y - (start.y - 60))).toBeLessThanOrEqual(1);
    expect(placed.width).toBe(820);

    await page.setViewportSize({ width: 800, height: 600 });
    await expect
      .poll(async () => onPage(await rectOf(composer), { width: 800, height: 600 }))
      .toBe(true);
    await page.setViewportSize({ width: 1280, height: 1000 });
    await expect.poll(() => rectOf(composer)).toEqual(placed);
  });

  test("the mail view's panes reflow with the page", async ({ page }) => {
    await page.setViewportSize({ width: 1280, height: 1000 });
    await openApp(page, server.url);
    const panes = page.locator('[data-node-id="panes"] > [data-part="split.pane"]');
    await expect(panes).toHaveCount(3);
    const widths = () =>
      panes.evaluateAll((els) => els.map((el) => Math.round(el.getBoundingClientRect().width)));
    const wide = await widths();
    await page.setViewportSize({ width: 1600, height: 1000 });
    await expect.poll(async () => (await widths())[2] ?? 0).toBeGreaterThan(wide[2] ?? 0);
    await page.setViewportSize({ width: 1000, height: 800 });
    await expect.poll(async () => (await widths()).reduce((a, b) => a + b, 0)).toBeLessThan(1000);
  });
});

test.describe("selection and right-clicks (ADRs 0182, 0184)", () => {
  test.beforeEach(async ({ page }) => {
    server = await startNail({ seed: "demo" });
    // Every right-click's fate, seen after Nail's handlers ran.
    await page.addInitScript(() => {
      const seen: boolean[] = [];
      Reflect.set(window, "nailContextMenus", seen);
      window.addEventListener("contextmenu", (event) => {
        seen.push(event.defaultPrevented);
      });
    });
  });

  /** Whether every right-click so far was kept from the browser. */
  const allPrevented = async (page: Page) =>
    page.evaluate(() => {
      const seen = Reflect.get(window, "nailContextMenus") as boolean[];
      return { count: seen.length, prevented: seen.every(Boolean) };
    });

  /** Drags across the first line of `locator` and answers what got selected. */
  async function dragSelect(page: Page, locator: Locator): Promise<string> {
    const box = defined(await locator.boundingBox(), "a box to drag across");
    await page.evaluate(() => window.getSelection()?.removeAllRanges());
    // Along the first line of text.
    const y = box.y + Math.min(10, box.height / 2);
    await page.mouse.move(box.x + 1, y);
    await page.mouse.down();
    await page.mouse.move(box.x + box.width - 1, y, { steps: 8 });
    await page.mouse.up();
    return page.evaluate(() => window.getSelection()?.toString() ?? "");
  }

  test("labels cannot be selected; the conversation and mail text can, and copy", async ({
    page,
  }) => {
    await openApp(page, server.url);
    await rows(page).filter({ hasText: "Welcome to Nail" }).click();
    await expect(page.locator(READER_SUBJECT)).toContainText("Welcome to Nail");
    const count = page.locator('[data-part="thread-view.count"]');
    const subject = page.locator('[data-part="thread-view.subject"]');
    const people = page.locator('[data-part="thread-view.participants"]');
    const body = page.locator('[data-part="message-view.text"]');
    const userSelect = (locator: Locator) =>
      locator.evaluate((el) => getComputedStyle(el).userSelect);
    // Nail's labels and controls: not selectable.
    expect(await userSelect(count)).toBe("none");
    expect(await userSelect(rows(page).first())).toBe("none");
    expect(await userSelect(rail(page))).toBe("none");
    expect(await userSelect(page.locator('[data-part="list-header.title"]').first())).toBe("none");
    // The user's data: selectable.
    for (const data of [subject, people, body]) expect(await userSelect(data)).toBe("text");
    await expect(page.getByRole("searchbox", { name: "Search mail" })).toHaveCSS(
      "user-select",
      "text",
    );

    expect(await dragSelect(page, count)).toBe("");
    const person = await dragSelect(page, people);
    expect(person.length).toBeGreaterThan(3);
    expect(await people.textContent()).toContain(person.trim());

    // A right-click on the selection: Nail's text menu; Copy copies it.
    await page.context().grantPermissions(["clipboard-read", "clipboard-write"]);
    const box = defined(await people.boundingBox(), "the participants");
    await page.mouse.click(box.x + 4, box.y + box.height / 2, { button: "right" });
    const menu = page.getByRole("menu", { name: "Edit" });
    await expect(menu).toBeVisible();
    await expect(menu.getByRole("menuitem")).toHaveText([/^Copy/, /^Select all/]);
    await expect(menu.getByRole("menuitem", { name: /Copy/ })).toContainText("Ctrl+C");
    await menu.getByRole("menuitem", { name: /Copy/ }).click();
    await expect(menu).toBeHidden();
    expect(await page.evaluate(() => navigator.clipboard.readText())).toBe(person);

    const text = await dragSelect(page, body);
    expect(text).toContain("Hi Sam");
    // Dragging across a list row selects no text (it picks the row).
    expect(await dragSelect(page, rows(page).first())).toBe("");
    expect(await allPrevented(page)).toEqual({ count: 1, prevented: true });
  });

  test("a right-click opens Nail's menu for the place or nothing, never the browser's", async ({
    page,
  }) => {
    await openApp(page, server.url);
    await expect(rows(page).first()).toBeVisible();
    const menu = page.getByRole("menu");
    const rightClick = async (locator: Locator, position?: { x: number; y: number }) => {
      await locator.click({ button: "right", ...(position ? { position } : {}) });
    };

    // The folder pane around its folders: Check for new mail, New message, Add account.
    const pane = page.locator('[data-part="folder-tree"]');
    const paneBox = defined(await pane.boundingBox(), "the folder pane");
    await rightClick(pane, { x: paneBox.width / 2, y: paneBox.height - 8 });
    await expect(page.getByRole("menu", { name: "Mail" }).getByRole("menuitem")).toHaveText([
      /^Check for new mail/,
      /^New message/,
      /^Add account/,
    ]);
    await page.keyboard.press("Escape");
    await expect(menu).toHaveCount(0);

    // A label of the list header: nothing.
    await rightClick(page.locator('[data-part="list-header.title"]').first());
    await expect(menu).toHaveCount(0);
    // The search field: the text menu.
    await rightClick(page.getByRole("searchbox", { name: "Search mail" }));
    await expect(page.getByRole("menu", { name: "Edit" })).toBeVisible();
    await page.keyboard.press("Escape");

    // A mail document in its frame: the text menu too.
    // (The search field's hints cover the list until it loses the focus.)
    await page.locator('[data-part="list-header.title"]').first().click();
    await rows(page).filter({ hasText: "Your weekly garden digest" }).click();
    await expect(page.locator(READER_SUBJECT)).toContainText("Your weekly garden digest");
    const frame = page.frameLocator('iframe[data-part="message-view.body"]');
    await frame.locator("h1").click({ button: "right" });
    await expect(page.getByRole("menu", { name: "Edit" }).getByRole("menuitem")).toHaveText([
      /^Select all/,
    ]);
    await page.keyboard.press("Escape");

    // The other views: whatever is right-clicked, the browser's menu never shows.
    for (const view of [
      "Calendar",
      "Contacts",
      "Tasks",
      "Settings",
    ] as const satisfies ViewName[]) {
      const region = await showView(page, view);
      await rightClick(region.locator("h1, h2, [role=heading]").first());
      await page.keyboard.press("Escape");
    }
    await rightClick(viewRegion(page, "Settings").getByRole("searchbox").first());
    await expect(page.getByRole("menu", { name: "Edit" })).toBeVisible();
    await page.keyboard.press("Escape");
    // The navigation rail: the View menu.
    await rightClick(rail(page));
    await expect(page.getByRole("menu", { name: "View" })).toBeVisible();
    await page.keyboard.press("Escape");

    const seen = await allPrevented(page);
    expect(seen.prevented).toBe(true);
    expect(seen.count).toBeGreaterThanOrEqual(9);
  });
});

test.describe("Check for new mail (ADR 0183)", () => {
  test.beforeEach(async () => {
    server = await startNail({ seed: "demo-multi" });
  });

  test("the button beside New message fetches new mail from the servers now", async ({ page }) => {
    await openApp(page, server.url);
    await sectionInbox(accountSectionNamed(page, "Home")).click();
    await expect(rows(page)).toHaveCount(3);
    const check = page.getByRole("button", { name: "Check for new mail" });
    await expect(check).toBeEnabled();
    // Beside New message, in the same row.
    const [newBox, checkBox] = [
      defined(await newMessageButton(page).boundingBox(), "New message"),
      defined(await check.boundingBox(), "Check for new mail"),
    ];
    expect(
      Math.abs(newBox.y + newBox.height / 2 - (checkBox.y + checkBox.height / 2)),
    ).toBeLessThan(2);
    expect(checkBox.x).toBeGreaterThan(newBox.x + newBox.width - 1);

    // POP3 has no IDLE: the message arrives only when Nail asks for it.
    await control(server, "pop3-deliver", {
      raw: "From: Grace Hopper <grace@home.example>\nTo: ada@home.example\nSubject: Fetched by the button\nMessage-ID: <button@home.example>\n\nHello!",
    });
    await check.click();
    await expect(rows(page).filter({ hasText: "Fetched by the button" })).toHaveCount(1);
    await expect(check).not.toHaveAttribute("data-state", "busy");

    // Ctrl+R in a text field checks too, and never reloads the app.
    await control(server, "pop3-deliver", {
      raw: "From: Grace Hopper <grace@home.example>\nTo: ada@home.example\nSubject: Fetched by Ctrl+R\nMessage-ID: <ctrl-r@home.example>\n\nHello!",
    });
    await page.evaluate(() => Reflect.set(window, "nailStillHere", true));
    await page.getByRole("searchbox", { name: "Search mail" }).focus();
    await page.keyboard.press("Control+R");
    await page.keyboard.press("F5");
    await expect(rows(page).filter({ hasText: "Fetched by Ctrl+R" })).toHaveCount(1);
    expect(await page.evaluate(() => Reflect.get(window, "nailStillHere") === true)).toBe(true);

    // Offline: off.
    await page.context().setOffline(true);
    await expect(check).toBeDisabled();
    await page.context().setOffline(false);
    await expect(check).toBeEnabled();
  });
});

test.describe("Reload window (ADR 0207)", () => {
  test.beforeEach(async () => {
    // The page's reports are info lines (ADR 0207).
    server = await startNail({ seed: "demo", log: `warn,${PAGE_LOG_TARGET}=info` });
  });

  /** The page's lines of the server's app.log. */
  function pageLines(): string[] {
    const file = path.join(server.dataDir, "logs", "app.log");
    if (!existsSync(file)) return [];
    return readFileSync(file, "utf8")
      .split(/\r?\n/)
      .filter((line) => line.includes(PAGE_LOG_TARGET));
  }
  const starts = () => pageLines().filter((line) => line.includes("page started")).length;

  test("Ctrl+Shift+R and the palette reload the window; each start is logged", async ({ page }) => {
    await openApp(page, server.url);
    await expect(rows(page).first()).toBeVisible();
    await expect.poll(starts).toBe(1);

    /** Runs `reload` and checks that the page was loaded again and logged its start. */
    const reloads = async (reload: () => Promise<void>, expectedStarts: number) => {
      await page.evaluate(() => Reflect.set(window, "nailStillHere", true));
      await reload();
      await expect
        .poll(() =>
          // The page may be mid-reload: no answer yet counts as "still here".
          page.evaluate(() => Reflect.get(window, "nailStillHere") === true).catch(() => true),
        )
        .toBe(false);
      await expect(rows(page).first()).toBeVisible();
      await expect.poll(starts).toBe(expectedStarts);
    };
    await reloads(() => page.keyboard.press("Control+Shift+R"), 2);
    await reloads(() => runFromPalette(page, "reload", "Reload window"), 3);

    // An uncaught error reaches the log too.
    await page.evaluate(() => {
      setTimeout(() => {
        throw new Error("thrown on purpose by the test");
      }, 0);
    });
    await expect
      .poll(() => pageLines().some((line) => line.includes("thrown on purpose by the test")))
      .toBe(true);
  });
});
