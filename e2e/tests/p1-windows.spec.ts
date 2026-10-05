import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { buildOutputDir, control, repoRoot, startNail, type NailServer } from "../support/nail";
import { createUser, rpcOk } from "../support/remote";
import {
  STANDARD_SIZES,
  formatIssues,
  layoutIssues,
  resizeWindow,
  screenshotPath,
  type CheckOptions,
  type ResponsiveIssue,
  type Size,
} from "../support/responsive";
import {
  newMessageButton,
  openSettings,
  runFromPalette,
  settingsNav,
  showView,
  toolbarAction,
} from "../support/views";

/**
 * P1 WP191: the responsive standard (ADR 0139, `docs/plan.md` "Wave P1")
 * on every window and dialog outside the mail and PIM views, in
 * `nail serve --seed demo-multi`: each one at the eight standard viewport
 * sizes, and each window-manager window also at its min-size, resized
 * larger and maximized. Every settings section is checked at the eight
 * sizes too.
 *
 * Coverage checks itself like `a11y-windows.spec.ts`: every layout window
 * and every feature dialog of that sweep is either in one of the
 * {@link GROUPS} below (and must have been checked there) or in
 * {@link OTHER_PACKAGES} (checked by `p1-mail.spec.ts` / `p1-views.spec.ts`).
 *
 * Every group writes what it found to
 * `<build dir>/screens/p1/windows-<group>.json` next to its screenshots.
 */

/**
 * The feature dialogs of the a11y window sweep, read from
 * `a11y-windows.spec.ts` itself (its `FEATURE_DIALOGS` list), so a dialog
 * added there fails here until a group below checks it.
 */
function featureDialogs(): string[] {
  const source = readFileSync(path.join(repoRoot, "e2e", "tests", "a11y-windows.spec.ts"), "utf8");
  const start = source.indexOf("const FEATURE_DIALOGS = [");
  const end = source.indexOf("] as const;", start);
  if (start < 0 || end < 0) throw new Error("FEATURE_DIALOGS not found in a11y-windows.spec.ts");
  // Quoted names in the list, comments left out.
  const list = source.slice(start, end).replace(/\/\/[^\n]*/g, "");
  return [...list.matchAll(/"([a-z0-9-]+)"/g)].map((m) => m[1] ?? "");
}

/**
 * Windows another P1 package checks: the main window, the mail view and the
 * PIM views with their dialogs (WP190 `p1-mail.spec.ts`, WP192
 * `p1-views.spec.ts`), and the M5d editors (WP192).
 */
const OTHER_PACKAGES = [
  "main",
  "mail",
  "calendar",
  "contacts",
  "tasks",
  "pim-event-editor",
  "pim-edit-scope",
  "pim-contact-editor",
  "pim-contact-groups",
  "pim-merge-contacts",
  "pim-task-editor",
  "pim-import",
  "pim-sources",
  "toolbar-customizer",
  "theme-editor",
  "theme-new",
  "theme-rename",
  "theme-delete",
] as const;

/** An Autocrypt Setup Message (the test corpus's). */
const setupMessage = () =>
  readFileSync(
    path.join(
      repoRoot,
      "crates",
      "nail-testkit",
      "corpus",
      "crypto",
      "pgp",
      "autocrypt-setup-bob.eml",
    ),
    "utf8",
  );

/** One checked state: its issues per size (and per window size). */
interface Finding {
  what: string;
  size: string;
  issues: ResponsiveIssue[];
}

/** What a check leaves out, each with its reason at the call. */
interface Options extends CheckOptions {
  /** Brings the page into the state to check after each resize. */
  prepare?: (page: Page) => Promise<void>;
  /** Issues that are fine by design (the caller says why). */
  drop?: (issue: ResponsiveIssue) => boolean;
}

/**
 * Panes inside `scope` that scroll sideways (`overflow-x: auto | scroll`
 * with wider content): the helper does not flag what such a pane hides, but
 * no form, list or card of a window is meant to scroll sideways — it must
 * wrap or shrink instead. Reported as `offscreen`.
 */
async function sidewaysScroll(
  page: Page,
  scope: string,
  ignore: readonly string[],
): Promise<ResponsiveIssue[]> {
  return page.evaluate(
    ({ scope: selector, ignore: skip }) => {
      const root = document.querySelector(selector);
      if (root === null) return [];
      const panes = [root, ...Array.from(root.querySelectorAll("[data-part]"))];
      return panes
        .filter((el) => {
          if (skip.some((s) => el.closest(s) !== null)) return false;
          const x = getComputedStyle(el).overflowX;
          return (x === "auto" || x === "scroll") && el.scrollWidth > el.clientWidth + 1;
        })
        .map((el) => ({
          kind: "offscreen" as const,
          element: `${el.getAttribute("data-part") ?? el.tagName.toLowerCase()} (scrolls sideways)`,
          detail: `content ${String(el.scrollWidth)}px wide in ${String(el.clientWidth)}px`,
        }));
    },
    { scope, ignore: [...ignore] },
  );
}

/**
 * Sticky rows (a dialog's button row, the composer's footer) cover the
 * content that scrolls beneath them by design. `unstick` first reports a
 * sticky row that is not wholly inside its window (it must stay visible),
 * then puts every sticky row of `scope` back into the flow for the overlap
 * and clipping checks; `restore` undoes that.
 */
async function stickyRows(
  page: Page,
  scope: string,
  action: "unstick" | "restore",
): Promise<ResponsiveIssue[]> {
  return page.evaluate(
    ({ selector, unstick }) => {
      const root = document.querySelector(selector);
      if (root === null) return [];
      const issues: { kind: "offscreen"; element: string; detail: string }[] = [];
      if (!unstick) {
        for (const el of Array.from(root.querySelectorAll<HTMLElement>("[data-p1-unstuck]"))) {
          el.style.removeProperty("position");
          el.removeAttribute("data-p1-unstuck");
        }
        return issues;
      }
      const box = root.getBoundingClientRect();
      const sticky = Array.from(root.querySelectorAll<HTMLElement>("[data-part]")).filter(
        (el) => getComputedStyle(el).position === "sticky",
      );
      // Two sticky rows must not cover each other (nested dialogs' button rows).
      sticky.forEach((a, i) => {
        const ra = a.getBoundingClientRect();
        for (const b of sticky.slice(i + 1)) {
          if (a.contains(b) || b.contains(a)) continue;
          const rb = b.getBoundingClientRect();
          const w = Math.min(ra.right, rb.right) - Math.max(ra.left, rb.left);
          const h = Math.min(ra.bottom, rb.bottom) - Math.max(ra.top, rb.top);
          if (w > 1 && h > 1) {
            issues.push({
              kind: "offscreen",
              element: `${a.getAttribute("data-part") ?? ""} / ${b.getAttribute("data-part") ?? ""} (sticky rows on top of each other)`,
              detail: `share ${String(Math.round(w))}×${String(Math.round(h))}px`,
            });
          }
        }
      });
      for (const el of sticky) {
        const r = el.getBoundingClientRect();
        if (r.height > 0 && (r.top < box.top - 1 || r.bottom > box.bottom + 1)) {
          issues.push({
            kind: "offscreen",
            element: `${el.getAttribute("data-part") ?? ""} (sticky row outside its window)`,
            detail: `${String(Math.round(r.top))}–${String(Math.round(r.bottom))} in ${String(Math.round(box.top))}–${String(Math.round(box.bottom))}`,
          });
        }
        el.style.position = "static";
        el.setAttribute("data-p1-unstuck", "");
      }
      return issues;
    },
    { selector: scope, unstick: action === "unstick" },
  );
}

/** The largest standard size; window resizing is checked there. */
const LARGE: Size = { width: 1600, height: 1000 };

/** Waits two frames (measured toolbars and lists settle). */
const settle = (page: Page) =>
  page.evaluate(
    () =>
      new Promise<void>((resolve) => {
        requestAnimationFrame(() => {
          requestAnimationFrame(() => {
            resolve();
          });
        });
      }),
  );

class Sweep {
  readonly checked = new Set<string>();
  readonly findings: Finding[] = [];
  readonly sections: string[] = [];

  constructor(
    readonly page: Page,
    readonly group: string,
  ) {}

  /** A dialog window (also an instance `<name>:<key>`). */
  window(name: string): Locator {
    return this.page
      .locator(
        `[data-part="window"][data-window="${name}"], [data-part="window"][data-window^="${name}:"]`,
      )
      .last();
  }

  private record(what: string, size: Size | string, issues: ResponsiveIssue[]) {
    const label = typeof size === "string" ? size : `${String(size.width)}×${String(size.height)}`;
    this.findings.push({ what, size: label, issues });
    expect
      .soft(
        issues,
        `${what} at ${label}:
${formatIssues(issues)}`,
      )
      .toEqual([]);
  }

  /** The helper's issues plus panes that scroll sideways, less the dropped ones. */
  async issues(scope: string, options: Options): Promise<ResponsiveIssue[]> {
    const hidden = await stickyRows(this.page, scope, "unstick");
    const found = [
      ...hidden,
      ...(await layoutIssues(this.page, { ...options, scope })),
      ...(await sidewaysScroll(this.page, scope, options.ignore ?? [])),
    ];
    await stickyRows(this.page, scope, "restore");
    return found.filter((issue) => !(options.drop?.(issue) ?? false));
  }

  /** The eight standard sizes of what `scope` selects. */
  async sizes(what: string, scope: string, options: Options = {}) {
    const page = this.page;
    for (const size of STANDARD_SIZES) {
      await page.setViewportSize(size);
      if (options.prepare) await options.prepare(page);
      await settle(page);
      this.record(what, size, await this.issues(scope, options));
      await page.screenshot({ path: screenshotPath(what, size) });
    }
    await page.setViewportSize(LARGE);
    await settle(page);
  }

  /**
   * Window `name`: the standard sizes, then its min-size, larger (1.5 × its
   * default, clamped by the viewport) and maximized, then it is recorded.
   */
  async checkWindow(name: string, what = name, options: Options = {}) {
    const frame = this.window(name);
    await expect(frame).toBeVisible();
    const id = await frame.getAttribute("data-window");
    const scope = `[data-part="window"][data-window="${id ?? name}"]`;
    await this.sizes(what, scope, options);
    await this.windowSizes(what, frame, scope, options);
    this.checked.add(name);
  }

  /** Min-size, larger and maximized at the large viewport. */
  async windowSizes(what: string, frame: Locator, scope: string, options: Options) {
    const page = this.page;
    await page.setViewportSize(LARGE);
    await settle(page);
    const handle = frame.locator(':scope > [data-part="window.handle"][data-edge="se"]');
    if ((await handle.count()) === 0) return; // Not resizable.
    const start = await frame.boundingBox();
    if (!start) throw new Error(`${what} is not visible`);
    const min = await resizeWindow(page, frame, { width: 1, height: 1 });
    await settle(page);
    this.record(
      what,
      `min-size ${String(min.width)}×${String(min.height)}`,
      await this.issues(scope, options),
    );
    await page.screenshot({ path: screenshotPath(`${what}-min`, min) });
    const larger = await resizeWindow(page, frame, {
      width: Math.round(start.width * 1.5),
      height: Math.round(start.height * 1.5),
    });
    await settle(page);
    this.record(
      what,
      `larger ${String(larger.width)}×${String(larger.height)}`,
      await this.issues(scope, options),
    );
    await page.screenshot({ path: screenshotPath(`${what}-larger`, larger) });
    const maximize = frame.locator('[data-part="window.maximize"]');
    if ((await maximize.count()) > 0 && (await maximize.isVisible())) {
      await maximize.click();
      await settle(page);
      this.record(what, "maximized", await this.issues(scope, options));
      await page.screenshot({ path: screenshotPath(`${what}-maximized`, LARGE) });
      await maximize.click();
      await settle(page);
    }
  }

  async close(name: string) {
    await this.window(name).locator('[data-part="window.close"]').click();
    await expect(this.window(name)).toBeHidden();
  }

  /** Writes the group's findings next to its screenshots. */
  write() {
    const dir = buildOutputDir(path.join("screens", "p1"));
    mkdirSync(dir, { recursive: true });
    writeFileSync(
      path.join(dir, `windows-${this.group}.json`),
      JSON.stringify(
        this.findings.filter((f) => f.issues.length > 0),
        null,
        2,
      ),
    );
  }
}

/** The settings view in the view host. */
const SETTINGS_VIEW = '[data-part="view-host.view"][data-window="settings"]';

interface Group {
  /** The windows and dialogs the group must check. */
  windows: readonly string[];
  run: (sweep: Sweep, server: NailServer) => Promise<void>;
}

const rowsOf = (page: Page) => page.locator('[data-part="message-list.row"]');

const GROUPS: Record<string, Group> = {
  mail: {
    windows: [
      "mail-delete-permanently",
      "composer",
      "composer-discard",
      "outbox",
      "rules",
      "account-setup",
      "snoozed",
      "snooze",
      "security-autocrypt-import",
      "print",
      "move",
      "sync-folder",
      "confirmation-center",
      "about-nail",
      "command-palette",
    ],
    run: async (sweep, server) => {
      const page = sweep.page;
      const rows = rowsOf(page);
      await rows.filter({ hasText: "Welcome to Nail" }).click();
      await page.keyboard.press("Shift+Delete");
      await sweep.checkWindow("mail-delete-permanently");
      await sweep.close("mail-delete-permanently");

      await newMessageButton(page).click();
      const composer = sweep.window("composer");
      await sweep.checkWindow("composer", "composer-plain");
      const bottom = (part: string) =>
        composer
          .locator(`[data-part="${part}"]`)
          .evaluate((el) => Math.round(el.getBoundingClientRect().bottom));
      const plainBottom = await bottom("composer.body");
      await composer.getByRole("radio", { name: "HTML" }).click();
      await expect(composer.locator('[data-part="composer.html.content"]')).toBeVisible();
      // P1 review: the rich editor fills the height down to the footer like the textarea.
      expect(Math.abs((await bottom("composer.html.content")) - plainBottom)).toBeLessThanOrEqual(
        16,
      );
      await sweep.sizes("composer-html", `[data-part="window"][data-window^="composer:"]`);
      await composer.getByRole("radio", { name: "Markdown" }).click();
      await expect(composer.locator('[data-part="composer.markdown.content"]')).toBeVisible();
      await sweep.sizes("composer-markdown", `[data-part="window"][data-window^="composer:"]`);
      await composer.getByRole("button", { name: "Discard" }).click();
      await sweep.checkWindow("composer-discard");
      await sweep.window("composer-discard").getByRole("button", { name: "Discard" }).click();
      await expect(page.locator('[data-part="window"][data-window^="composer"]')).toHaveCount(0);

      await runFromPalette(page, "outbox", "Outbox");
      await sweep.checkWindow("outbox");
      await sweep.close("outbox");

      await runFromPalette(page, "filter rules", "Filter rules");
      await sweep.checkWindow("rules", "rules-list");
      await sweep.window("rules").getByRole("button", { name: "New rule" }).click();
      await expect(sweep.window("rules").getByRole("form", { name: "Rule" })).toBeVisible();
      await sweep.checkWindow("rules", "rules-editor");
      await sweep.close("rules");

      await runFromPalette(page, "add account", "Add account");
      await sweep.checkWindow("account-setup");
      await sweep.close("account-setup");

      await runFromPalette(page, "snoozed", "Snoozed messages");
      await sweep.checkWindow("snoozed");
      await sweep.close("snoozed");

      await rows.filter({ hasText: "Welcome to Nail" }).click();
      const actions = page.getByRole("toolbar", { name: "Message actions" });
      await toolbarAction(page, actions, "Snooze…");
      await sweep.checkWindow("snooze");
      await sweep.close("snooze");
      await control(server, "imap-deliver", { raw: setupMessage() });
      const setup = rows.filter({ hasText: "Autocrypt Setup Message" }).first();
      await expect(setup).toBeVisible({ timeout: 20_000 });
      await setup.click();
      await toolbarAction(page, actions, "Import Autocrypt setup message");
      await sweep.checkWindow("security-autocrypt-import");
      await sweep.close("security-autocrypt-import");
      await toolbarAction(page, actions, "Print the message…");
      await sweep.checkWindow("print");
      await sweep.close("print");

      await rows.filter({ hasText: "Welcome to Nail" }).click({ button: "right" });
      const menu = page.getByRole("menu");
      await expect(menu).toBeVisible();
      await menu.getByRole("menuitem", { name: "Move to…" }).click();
      await sweep.checkWindow("move");
      await sweep.close("move");
      await page
        .getByRole("treeitem", { name: /^Inbox/ })
        .first()
        .click({ button: "right" });
      await page.getByRole("menu").getByRole("menuitem", { name: "Synchronization…" }).click();
      await sweep.checkWindow("sync-folder");
      await sweep.close("sync-folder");

      await page.keyboard.press("g");
      await page.keyboard.press("a");
      await sweep.checkWindow("confirmation-center");
      await sweep.close("confirmation-center");

      await page.keyboard.press("F1");
      await sweep.checkWindow("about-nail");
      await sweep.close("about-nail");

      await page.keyboard.press("Control+k");
      await sweep.checkWindow("command-palette");
      await page.keyboard.press("Escape");
      await expect(sweep.window("command-palette")).toBeHidden();
    },
  },
  settings: {
    windows: ["settings"],
    run: async (sweep) => {
      const page = sweep.page;
      const view = await openSettings(page);
      const names = await settingsNav(view)
        .getByRole("button")
        .evaluateAll((items) => items.map((item) => item.textContent.trim()));
      expect(names.length, "settings sections").toBeGreaterThan(0);
      for (const name of names) {
        await openSettings(page, name);
        await sweep.sizes(`settings-${name}`, SETTINGS_VIEW, {
          prepare: async (p) => {
            await expect(p.locator('[data-part="settings-page.title"]')).toHaveText(name);
          },
        });
        // Rows of controls (a key list, a device list) draw no browser bullets (P1 review).
        expect(await bulletedRows(page.locator(SETTINGS_VIEW)), `bullets in ${name}`).toEqual([]);
        sweep.sections.push(name);
      }
      sweep.checked.add("settings");
    },
  },
  "settings-dialogs": {
    windows: [
      "remote-pair",
      "config-merge",
      "config-errors",
      "import-wizard",
      "export-wizard",
      "backup-wizard",
      "restore-wizard",
      "diagnostics-bundle",
      "plugin-install",
    ],
    run: async (sweep) => {
      const page = sweep.page;
      const settings = page.locator(SETTINGS_VIEW);
      await openSettings(page, "Remote access");
      await settings.getByRole("button", { name: "Pair a device" }).first().click();
      await sweep.checkWindow("remote-pair");
      await sweep.close("remote-pair");
      await openSettings(page, "Layout & toolbars");
      const customize = page.getByRole("toolbar", { name: "Customization" });
      await customize.getByRole("button", { name: "Apply a theme or layout…" }).click();
      await sweep.checkWindow("config-merge");
      await sweep.close("config-merge");
      await customize.getByRole("button", { name: "Configuration errors" }).click();
      await sweep.checkWindow("config-errors");
      await sweep.close("config-errors");
      await openSettings(page, "Data & backup");
      for (const [button, name] of [
        ["Import…", "import-wizard"],
        ["Export…", "export-wizard"],
        ["Back up…", "backup-wizard"],
        ["Restore…", "restore-wizard"],
        ["Create diagnostics bundle…", "diagnostics-bundle"],
      ] as const) {
        await settings.getByRole("button", { name: button }).click();
        await sweep.checkWindow(name);
        await sweep.close(name);
      }
      await openSettings(page, "Plug-ins");
      await settings.getByRole("button", { name: "Install a plug-in…" }).click();
      await sweep.checkWindow("plugin-install");
      await sweep.close("plugin-install");
    },
  },
  scheduling: {
    windows: ["scheduling-cancel-meeting", "scheduling-find-time", "scheduling-settings"],
    run: async (sweep) => {
      const page = sweep.page;
      await showView(page, "Calendar");
      const calendar = page.locator('[data-part="view-host.view"][data-window="calendar"]');
      await calendar.getByRole("button", { name: "Agenda", exact: true }).click();
      await calendar
        .getByRole("button", { name: /^Project kickoff,/ })
        .first()
        .click();
      const meeting = sweep
        .window("pim-event-editor")
        .getByRole("region", { name: "Meeting: Project kickoff" });
      await expect(meeting).toBeVisible();
      await meeting.getByRole("button", { name: "Cancel meeting…" }).click();
      await sweep.checkWindow("scheduling-cancel-meeting");
      await sweep.close("scheduling-cancel-meeting");
      await sweep.close("pim-event-editor");
      const calendarActions = page.getByRole("toolbar", { name: "Calendar actions" });
      for (const [button, name] of [
        ["Find a time", "scheduling-find-time"],
        ["Invitation settings", "scheduling-settings"],
      ] as const) {
        await toolbarAction(page, calendarActions, button);
        await sweep.checkWindow(name);
        await sweep.close(name);
      }
    },
  },
  gmail: {
    windows: ["gmail-label-editor", "gmail-label-delete", "gmail-apply-labels"],
    run: async (sweep) => {
      const page = sweep.page;
      const settings = await openSettings(page, "Accounts");
      await settings.getByRole("button", { name: /^Gmail/ }).click();
      await settings.getByRole("checkbox", { name: "Synchronize this account" }).check();
      await settings.getByRole("button", { name: "Save", exact: true }).click();
      await expect(settings.getByText("Settings saved.")).toBeVisible();
      await showView(page, "Mail");
      const gmail = page
        .locator('[data-part="folder-tree.section"][data-account-kind="gmail"]')
        .first();
      await expect(gmail).toHaveAttribute("data-status", "online", { timeout: 30_000 });
      await gmail.locator('[data-part="folder-tree.item"][data-role="inbox"]').first().click();
      const rows = rowsOf(page);
      await expect(rows).toHaveCount(4, { timeout: 30_000 });
      const labelList = page.locator('[data-part="label-list"]');
      await labelList.getByRole("button", { name: "New label" }).click();
      await sweep.checkWindow("gmail-label-editor", "gmail-label-editor-new");
      await sweep.close("gmail-label-editor");
      await labelList.getByRole("button", { name: "Edit label Work/Projects" }).click();
      await sweep.checkWindow("gmail-label-editor", "gmail-label-editor-existing");
      await sweep.close("gmail-label-editor");
      await labelList.getByRole("button", { name: "Delete label Work/Projects" }).click();
      await sweep.checkWindow("gmail-label-delete");
      await sweep.close("gmail-label-delete");
      await rows.filter({ hasText: "Lunch on Friday?" }).click();
      await toolbarAction(page, page.getByRole("toolbar", { name: "Message actions" }), "Labels…");
      await sweep.checkWindow("gmail-apply-labels");
      await sweep.close("gmail-apply-labels");
    },
  },
};

// Fail a missing control fast instead of after the whole test budget.
test.use({ actionTimeout: 20_000 });

/**
 * The parts of list items under `scope` that hold a control but still show a
 * list marker (a browser bullet beside a row of the list).
 */
function bulletedRows(scope: Locator): Promise<string[]> {
  return scope.evaluate((root) =>
    Array.from(root.querySelectorAll("ul > li, ol > li"))
      .filter((li) => {
        const style = getComputedStyle(li);
        return (
          style.display === "list-item" &&
          style.listStyleType !== "none" &&
          li.querySelector("button, input, select, a[href]") !== null
        );
      })
      .map((li) => li.parentElement?.getAttribute("data-part") ?? "(no part)"),
  );
}

test.describe("P1: every window and dialog is responsive", () => {
  let server: NailServer;

  test.beforeEach(async () => {
    server = await startNail({ seed: "demo-multi" });
    // A remote user, so "Pair a device" can be opened.
    await createUser(server, "ada");
  });

  test.afterEach(async () => {
    await server.stop();
  });

  test("the groups cover every window of the a11y sweep", async () => {
    const config = await rpcOk(server, { type: "GetConfig" }, "Config");
    const layout = config.layout as { windows: Record<string, unknown> };
    const dialogs = featureDialogs();
    expect(dialogs.length, "dialogs of the a11y sweep").toBeGreaterThan(30);
    const expected = [...new Set([...Object.keys(layout.windows), ...dialogs])].sort();
    const covered = [
      ...new Set([...Object.values(GROUPS).flatMap((g) => g.windows), ...OTHER_PACKAGES]),
    ].sort();
    expect(covered).toEqual(expected);
  });

  for (const [group, { windows, run }] of Object.entries(GROUPS)) {
    test(group, async ({ page }) => {
      test.setTimeout(600_000);
      const sweep = new Sweep(page, group);
      await page.setViewportSize(LARGE);
      await page.goto(server.url);
      await expect(rowsOf(page)).toHaveCount(17);
      try {
        await run(sweep, server);
      } finally {
        sweep.write();
      }
      expect([...sweep.checked].sort(), `${group}: windows checked`).toEqual([...windows].sort());
      if (group === "settings") {
        const config = await rpcOk(server, { type: "GetConfig" }, "Config");
        const layout = config.layout as { settings: { sections: unknown[] }[] };
        expect(sweep.sections.length, `sections: ${sweep.sections.join(", ")}`).toBe(
          layout.settings.reduce((sum, g) => sum + g.sections.length, 0),
        );
      }
    });
  }
});
