import { expect, test, type Locator, type Page } from "@playwright/test";
import { startNail, type NailServer } from "../support/nail";
import {
  expectResponsive,
  formatIssues,
  layoutIssues,
  resizeWindow,
  screenshotPath,
  type CheckOptions,
} from "../support/responsive";
import { openApp, openSettings, readerToolbar, runFromPalette, showView } from "../support/views";

/**
 * Wave P1 WP192 (ADR 0139's responsive standard) against `nail serve`: the
 * calendar (every view), contacts and tasks views, the navigation rail and
 * view host around them, customize-layout mode, the toolbar customizer,
 * Settings → Appearance and the theme editor — at every standard viewport
 * size, and the two editor windows at their min-size and resized larger.
 * Plus the M5 cosmetic list: the theme editor names itself once, the
 * customizer's italic "Separator" is not cut off, the "This theme" tags
 * line up.
 *
 * Screenshots: `<build dir>/screens/p1/views-*.png`.
 */

let server: NailServer | undefined;

test.afterEach(async () => {
  await server?.stop();
  server = undefined;
});

async function open(page: Page, seed: "demo" | "demo-multi" = "demo") {
  server = await startNail({ seed });
  await openApp(page, server.url);
}

/** Every calendar view, by its label (the switch or, when it does not fit, the menu). */
const CALENDAR_VIEWS = ["Day", "Week", "Month", "Year", "Agenda", "Free time"] as const;

/** Shows a calendar view through the segmented switch or the narrow view menu. */
async function chooseCalendarView(calendar: Locator, name: string): Promise<void> {
  const group = calendar.getByRole("group", { name: "View", exact: true });
  const menu = calendar.getByRole("combobox", { name: "View", exact: true });
  await expect(group.or(menu).first()).toBeVisible();
  if (await menu.isVisible()) {
    await menu.selectOption({ label: name });
    return;
  }
  const button = group.getByRole("button", { name, exact: true });
  if ((await button.count()) > 0 && (await button.isVisible())) {
    await button.click();
    await expect(button).toHaveAttribute("aria-pressed", "true");
    return;
  }
  // A view the switch moved into its "More" menu.
  await group.getByRole("button", { name: "More", exact: true }).click();
  await calendar.page().getByRole("menuitem", { name, exact: true }).click();
}

/** Closes a window of the window manager when it is open. */
async function closeWindow(dialog: Locator): Promise<void> {
  if (!(await dialog.isVisible())) return;
  // The close button may sit outside a viewport that just shrank.
  await dialog.locator('[data-part="window.close"]').first().dispatchEvent("click");
  await expect(dialog).toBeHidden();
}

const windowFrame = (page: Page, id: string) =>
  page.locator(`[data-part="window"][data-window="${id}"]`);

/**
 * A window at its min-size and resized larger (1.5 × its default size,
 * within the viewport): no layout issue inside it, screenshots of both.
 */
async function expectWindowSizes(
  page: Page,
  frame: Locator,
  name: string,
  options: CheckOptions = {},
): Promise<void> {
  await page.setViewportSize({ width: 1280, height: 1000 });
  const start = await frame.boundingBox();
  if (!start) throw new Error(`${name} is not visible`);
  const scope = `[data-part="window"][data-window="${String(await frame.getAttribute("data-window"))}"]`;
  const sizes = [
    { width: 1, height: 1 },
    { width: Math.round(start.width * 1.5), height: Math.round(start.height * 1.5) },
  ];
  for (const wanted of sizes) {
    const got = await resizeWindow(page, frame, wanted);
    // Let the measured parts (toolbars, container queries) settle.
    await page.evaluate(
      () =>
        new Promise<void>((resolve) => {
          requestAnimationFrame(() => {
            requestAnimationFrame(() => {
              resolve();
            });
          });
        }),
    );
    const issues = await layoutIssues(page, { ...options, scope });
    const label = wanted.width === 1 ? "min" : "large";
    await page.screenshot({ path: screenshotPath(`${name}-${label}`, got) });
    expect(
      issues,
      `${name} at ${label} (${String(got.width)}×${String(got.height)}):\n${formatIssues(issues)}`,
    ).toEqual([]);
  }
}

test("the calendar's views and the rail at every size", async ({ page }) => {
  await open(page, "demo-multi");
  for (const view of CALENDAR_VIEWS) {
    await expectResponsive(page, `views-calendar-${view.toLowerCase().replace(" ", "-")}`, {
      // The all-day row's day cells are the "new all-day event" click
      // targets behind the all-day chips (like the time grid's slots behind
      // its events): they lie under the chips on purpose.
      ignore: ['[data-part="calendar.all-day.cell"]'],
      prepare: async () => {
        const calendar = await showView(page, "Calendar");
        await chooseCalendarView(calendar, view);
      },
    });
  }
});

test("contacts and tasks at every size", async ({ page }) => {
  await open(page, "demo-multi");
  await expectResponsive(page, "views-contacts", {
    prepare: async () => {
      const contacts = await showView(page, "Contacts");
      const first = contacts.locator('[data-part="contact-list.item"]').first();
      if ((await first.count()) > 0) await first.click();
    },
  });
  await expectResponsive(page, "views-tasks", {
    prepare: async () => {
      await showView(page, "Tasks");
    },
  });
});

test("customize-layout mode at every size", async ({ page }) => {
  await open(page);
  await expectResponsive(page, "views-layout-editor-mail", {
    prepare: async () => {
      const editor = page.getByRole("region", { name: "Customizing layout: Mail" });
      if (!(await editor.isVisible())) {
        await runFromPalette(page, "customize layout", "Customize layout");
      }
      await expect(editor).toBeVisible();
    },
  });

  // At 1024 px the palette leaves the mail window less room than its panes
  // need: the canvas scrolls sideways to the last pane instead of cutting it.
  await page.setViewportSize({ width: 1024, height: 700 });
  const canvas = page.locator('[data-part="layout-editor.canvas"]');
  await canvas.evaluate((el) => {
    el.scrollLeft = el.scrollWidth;
  });
  const handle = page.getByRole("button", { name: "Move Message", exact: true });
  await expect(handle).toBeInViewport({ ratio: 1 });
  const canvasBox = await canvas.boundingBox();
  const handleBox = await handle.boundingBox();
  expect(
    handleBox && canvasBox && handleBox.x + handleBox.width <= canvasBox.x + canvasBox.width,
  ).toBe(true);
});

test("the toolbar customizer at every size, min-size and larger", async ({ page }) => {
  await open(page);
  const dialog = page.getByRole("dialog", { name: "Customize toolbar" });
  const prepare = async () => {
    // Opened afresh at each size: an open window keeps its place when the
    // viewport shrinks (window manager, see the report), a new one opens
    // inside the page.
    await closeWindow(dialog);
    await showView(page, "Mail");
    // Below 900 px the mail window shows its panes as tabs.
    const tab = (name: string) => page.getByRole("tab", { name, exact: true });
    if (await tab("Messages").isVisible()) await tab("Messages").click();
    await page.locator('[data-part="message-list.row"]').first().click();
    if (await tab("Message").isVisible()) await tab("Message").click();
    const bar = readerToolbar(page);
    await bar.getByRole("button").first().click({ button: "right" });
    await page
      .getByRole("menu", { name: "Toolbar" })
      .getByRole("menuitem", { name: "Customize toolbar" })
      .click();
    await expect(dialog.getByRole("listbox", { name: "Toolbar" })).toBeVisible();
  };
  await expectResponsive(page, "views-toolbar-customizer", { prepare });

  // The italic "Separator" is not cut off (M5 cosmetic list).
  const separator = dialog
    .locator('[data-part="toolbar-customizer.item"][data-kind="separator"]')
    .locator('[data-part="toolbar-customizer.item.label"]')
    .first();
  await expect(separator).toBeVisible();
  const cut = await separator.evaluate((el) => {
    const range = document.createRange();
    range.selectNodeContents(el);
    const text = range.getBoundingClientRect();
    const box = el.getBoundingClientRect();
    // The glyphs' ink of an italic overhangs the text box by a bit: the
    // label keeps room for it after the text (its end padding).
    return { overflow: el.scrollWidth - el.clientWidth, room: box.right - text.right };
  });
  expect(cut.overflow).toBeLessThanOrEqual(0);
  expect(cut.room).toBeGreaterThanOrEqual(2);

  await expectWindowSizes(
    page,
    windowFrame(page, (await dialog.getAttribute("data-window")) ?? ""),
    "views-toolbar-customizer",
  );
});

test("Settings → Appearance at every size", async ({ page }) => {
  await open(page);
  await expectResponsive(page, "views-appearance", {
    prepare: async () => {
      await openSettings(page, "Appearance");
    },
  });

  // The P1 verify finding: between 1024 and 1150 px the "Light theme" /
  // "Dark theme" labels ran under their selects ("Light t", "Dark th").
  for (const width of [1024, 1080, 1150, 1200]) {
    await page.setViewportSize({ width, height: 800 });
    await openSettings(page, "Appearance");
    const pair = page.locator('[data-part="appearance.pair"]');
    await expect(pair).toBeVisible();
    const fields = await pair.locator(':scope > [data-part="form.field"]').evaluateAll((els) =>
      els.map((field) => {
        const label = field.querySelector(':scope > [data-part="form.label"]');
        const select = field.querySelector(':scope > [data-part="form.select"]');
        if (!label || !select) return { name: "", textRight: 0, labelRight: 0, selectLeft: 0 };
        const range = document.createRange();
        range.selectNodeContents(label);
        const box = select.getBoundingClientRect();
        const text = range.getBoundingClientRect();
        return {
          name: label.textContent.trim(),
          textRight: text.right,
          labelRight: label.getBoundingClientRect().right,
          // A select below its label (stacked) does not compete for the line.
          selectLeft: box.top >= text.bottom ? Number.POSITIVE_INFINITY : box.left,
        };
      }),
    );
    expect(fields.map((f) => f.name)).toEqual(["Light theme", "Dark theme"]);
    for (const field of fields) {
      expect(field.textRight, `${field.name} at ${String(width)} px`).toBeLessThanOrEqual(
        field.labelRight + 1,
      );
      expect(field.textRight, `${field.name} at ${String(width)} px`).toBeLessThanOrEqual(
        field.selectLeft,
      );
    }
    const issues = await layoutIssues(page, { scope: '[data-part="appearance.pair"]' });
    expect(issues, formatIssues(issues)).toEqual([]);
  }
});

test("the theme editor at every size, min-size and larger; its title once, tags aligned", async ({
  page,
}) => {
  await open(page);
  const dialog = page.getByRole("dialog", { name: "Theme editor" });
  const prepare = async () => {
    await closeWindow(dialog);
    await runFromPalette(page, "edit theme", "Edit theme");
    await expect(dialog.locator('[data-part="theme-editor.token"]').first()).toBeVisible();
  };
  await expectResponsive(page, "views-theme-editor", { prepare });

  // The title shows once: in the window's title bar.
  await expect(dialog.getByText("Theme editor", { exact: true })).toHaveCount(1);

  // Every "This theme" / "Base theme" tag sits in one column and on one line.
  const tags = dialog.locator(
    '[data-part="theme-editor.token"] [data-part="tag"][data-kind="origin"]',
  );
  const boxes = await tags.evaluateAll((els) =>
    els
      .map((el) => el.getBoundingClientRect())
      .filter((r) => r.width > 0)
      .map((r) => ({ right: Math.round(r.right), height: Math.round(r.height) })),
  );
  expect(boxes.length).toBeGreaterThan(3);
  const first = boxes[0];
  for (const box of boxes) {
    expect(box.right).toBe(first?.right);
    expect(box.height).toBe(first?.height);
  }

  // The Parts tab too.
  await dialog.getByRole("tab", { name: "Parts" }).click();
  await expectResponsive(page, "views-theme-editor-parts", {
    prepare: async () => {
      await prepare();
      const parts = dialog.getByRole("tab", { name: "Parts" });
      if ((await parts.getAttribute("aria-selected")) !== "true") await parts.click();
    },
  });

  await dialog.getByRole("tab").first().click();
  await expectWindowSizes(page, windowFrame(page, "theme-editor"), "views-theme-editor");
});
