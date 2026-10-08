import { mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { buildOutputDir, nailPerTest, type NailServer } from "../support/nail";
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
  DEMO_MULTI_CONVERSATIONS,
  messageRows,
  newMessageButton,
  nextFrames,
  openMail,
  openSettings,
  runFromPalette,
  showView,
  syncAccount,
  toolbarAction,
  windowSelector,
} from "../support/views";
import {
  contextMenusStep,
  deletePermanentlyStep,
  discardComposerStep,
  editScopeStep,
  FEATURE_DIALOGS,
  gmailLabelsStep,
  keyWindowsStep,
  meetingStep,
  messageDialogsStep,
  paletteWindowStep,
  settingsDialogsStep,
  settingsSectionNames,
  WindowTour,
  calendarActions,
  PIM_ACTION_WINDOWS,
  SCHEDULING_ACTION_WINDOWS,
  showAgenda,
  toolbarWindowsStep,
} from "../support/windowTour";

/**
 * P1 WP191: the responsive standard (ADR 0139, `docs/plan.md` "Wave P1")
 * on every window and dialog outside the mail and PIM views, in
 * `nail serve --seed demo-multi`: each one at the eight standard viewport
 * sizes, and each window-manager window also at its min-size, resized
 * larger and maximized. Every settings section is checked at the eight
 * sizes too.
 *
 * Coverage checks itself like `a11y-windows.spec.ts`: every layout window
 * and every feature dialog of the tour (`FEATURE_DIALOGS`) is either in one of the
 * {@link GROUPS} below (and must have been checked there) or in
 * {@link OTHER_PACKAGES} (checked by `p1-mail.spec.ts` / `p1-views.spec.ts`).
 *
 * Every group writes what it found to
 * `<build dir>/screens/p1/windows-<group>.json` next to its screenshots.
 */

/**
 * Windows another P1 package checks: the main window, the mail view and the
 * PIM views (WP190 `p1-mail.spec.ts`, WP192 `p1-views.spec.ts`), and the M5d
 * editors (WP192). The PIM dialogs are the `pim` group's below (ADR 0160).
 */
const OTHER_PACKAGES = [
  "main",
  "mail",
  "calendar",
  "contacts",
  "tasks",
  "toolbar-customizer",
  "theme-editor",
  "theme-new",
  "theme-rename",
  "theme-delete",
] as const;

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

/**
 * ADR 0160: the controls of one form column line up — every one starts at
 * the same left edge and ends at the same right edge (± 2 px), at the
 * current viewport size. `controls` are checked in the given order.
 */
async function expectLinedUp(what: string, controls: readonly Locator[]): Promise<void> {
  const boxes = [];
  for (const control of controls) {
    await expect(control).toBeVisible();
    const box = await control.boundingBox();
    if (!box) throw new Error(`${what}: a control has no box`);
    boxes.push({
      name: (await control.getAttribute("aria-label")) ?? (await control.evaluate((el) => el.id)),
      left: Math.round(box.x),
      right: Math.round(box.x + box.width),
    });
  }
  const first = boxes[0];
  if (first === undefined) throw new Error(`${what}: no controls`);
  const off = boxes.filter(
    (b) => Math.abs(b.left - first.left) > 2 || Math.abs(b.right - first.right) > 2,
  );
  expect(off, `${what}: ${JSON.stringify(boxes)}`).toEqual([]);
}

/** Lines the controls up at the large, a medium and the narrow standard width. */
async function expectLinedUpAtWidths(
  page: Page,
  what: string,
  controls: () => readonly Locator[],
): Promise<void> {
  for (const width of [1600, 1024, 800]) {
    await page.setViewportSize({ width, height: 1000 });
    await nextFrames(page);
    await expectLinedUp(`${what} at ${String(width)} px`, controls());
  }
  await page.setViewportSize(LARGE);
  await nextFrames(page);
}

class Sweep extends WindowTour {
  readonly checked = new Set<string>();
  readonly findings: Finding[] = [];
  readonly sections: string[] = [];

  constructor(
    page: Page,
    readonly group: string,
  ) {
    super(page);
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
      await nextFrames(page);
      this.record(what, size, await this.issues(scope, options));
      await page.screenshot({ path: screenshotPath(what, size) });
    }
    await page.setViewportSize(LARGE);
    await nextFrames(page);
  }

  /**
   * Window `name`: the standard sizes, then its min-size, larger (1.5 × its
   * default, clamped by the viewport) and maximized, then it is recorded.
   */
  async visit(name: string, what = name, options: Options = {}) {
    const frame = this.window(name);
    await expect(frame).toBeVisible();
    const id = await frame.getAttribute("data-window");
    const scope = windowSelector(id ?? name);
    await this.sizes(what, scope, options);
    await this.windowSizes(what, frame, scope, options);
    this.checked.add(name);
  }

  /** Min-size, larger and maximized at the large viewport. */
  async windowSizes(what: string, frame: Locator, scope: string, options: Options) {
    const page = this.page;
    await page.setViewportSize(LARGE);
    await nextFrames(page);
    const handle = frame.locator(':scope > [data-part="window.handle"][data-edge="se"]');
    if ((await handle.count()) === 0) return; // Not resizable.
    const start = await frame.boundingBox();
    if (!start) throw new Error(`${what} is not visible`);
    const min = await resizeWindow(page, frame, { width: 1, height: 1 });
    await nextFrames(page);
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
    await nextFrames(page);
    this.record(
      what,
      `larger ${String(larger.width)}×${String(larger.height)}`,
      await this.issues(scope, options),
    );
    await page.screenshot({ path: screenshotPath(`${what}-larger`, larger) });
    const maximize = frame.locator('[data-part="window.maximize"]');
    if ((await maximize.count()) > 0 && (await maximize.isVisible())) {
      await maximize.click();
      await nextFrames(page);
      this.record(what, "maximized", await this.issues(scope, options));
      await page.screenshot({ path: screenshotPath(`${what}-maximized`, LARGE) });
      await maximize.click();
      await nextFrames(page);
    }
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
      const rows = messageRows(page);
      // P3: "Delete permanently?" asks only in Trash.
      await deletePermanentlyStep(sweep, "Your weekly garden digest");
      await rows.filter({ hasText: "Welcome to Nail" }).click();

      await newMessageButton(page).click();
      const composer = sweep.window("composer");
      await sweep.visit("composer", "composer-plain");
      const bottom = (part: string) =>
        composer
          .locator(`[data-part="${part}"]`)
          .evaluate((el) => Math.round(el.getBoundingClientRect().bottom));
      const plainBottom = await bottom("composer.body");
      // P1 verify: the To input spans its row like Subject (it was the browser's
      // 20 characters wide and cut "Ben Ortiz <ben@nail.example>").
      const right = (name: string) =>
        composer
          .getByRole("textbox", { name, exact: true })
          .evaluate((el) => Math.round(el.getBoundingClientRect().right));
      expect(Math.abs((await right("To")) - (await right("Subject")))).toBeLessThanOrEqual(2);
      // ADR 0160: with Cc and Bcc shown, To / Cc / Bcc / Subject share both edges at every width.
      await composer.getByRole("button", { name: "Add Cc" }).click();
      await composer.getByRole("button", { name: "Add Bcc" }).click();
      const textbox = (name: string) => composer.getByRole("textbox", { name, exact: true });
      await expectLinedUpAtWidths(page, "composer To / Cc / Bcc / Subject", () =>
        ["To", "Cc", "Bcc", "Subject"].map(textbox),
      );
      await sweep.sizes("composer-cc-bcc", `[data-part="window"][data-window^="composer:"]`);
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
      await discardComposerStep(sweep);
      await paletteWindowStep(sweep, "outbox", "Outbox", "outbox");

      await runFromPalette(page, "filter rules", "Filter rules");
      await sweep.visit("rules", "rules-list");
      await sweep.window("rules").getByRole("button", { name: "New rule" }).click();
      await expect(sweep.window("rules").getByRole("form", { name: "Rule" })).toBeVisible();
      await sweep.visit("rules", "rules-editor");
      // ADR 0160: a rule with conditions and actions — every control of a row fills its row.
      const rulesWindow = sweep.window("rules");
      const addCondition = rulesWindow.getByRole("button", { name: "Add condition" });
      await addCondition.click();
      // The "add" row's choosers (each added row has its own selects).
      const adders = rulesWindow.locator('[data-part="rules.add"]');
      await adders
        .first()
        .getByRole("combobox", { name: "Condition" })
        .selectOption({ label: "Subject" });
      await addCondition.click();
      await rulesWindow.getByRole("button", { name: "Add action" }).click();
      await adders.last().getByRole("combobox", { name: "Action" }).selectOption({ index: 2 });
      await rulesWindow.getByRole("button", { name: "Add action" }).click();
      await sweep.visit("rules", "rules-editor-filled");
      // The condition group's rows span its Match select: the first condition's
      // controls together, and the "add" row's chooser with its button.
      for (const width of [1600, 1024, 800]) {
        await page.setViewportSize({ width, height: 1000 });
        await nextFrames(page);
        const edges = await rulesWindow
          .locator('[data-part="rules.group"]')
          .first()
          .evaluate((group) => {
            const span = (els: Element[]) => {
              const rects = els.map((el) => el.getBoundingClientRect());
              return {
                left: Math.round(Math.min(...rects.map((r) => r.left))),
                right: Math.round(Math.max(...rects.map((r) => r.right))),
              };
            };
            const controls = (row: Element | null) =>
              row === null
                ? []
                : Array.from(row.querySelectorAll("select, input:not([type=checkbox]), button"));
            const match = group.querySelector("select");
            return {
              match: match === null ? null : span([match]),
              condition: span(controls(group.querySelector('[data-part="rules.condition"]'))),
              add: span(controls(group.querySelector('[data-part="rules.add"]'))),
            };
          });
        const near = (
          a: { left: number; right: number } | null,
          b: { left: number; right: number } | null,
        ) =>
          a !== null &&
          b !== null &&
          Math.abs(a.left - b.left) <= 2 &&
          Math.abs(a.right - b.right) <= 2;
        expect(
          near(edges.condition, edges.match),
          `rule condition at ${String(width)} px: ${JSON.stringify(edges)}`,
        ).toBe(true);
        expect(
          near(edges.add, edges.match),
          `rule "add" row at ${String(width)} px: ${JSON.stringify(edges)}`,
        ).toBe(true);
      }
      await page.setViewportSize(LARGE);
      await sweep.close("rules");

      await runFromPalette(page, "add account", "Add account");
      await sweep.visit("account-setup");
      // ADR 0160: the server step (settings to check by hand) with its server fields.
      const setupWindow = sweep.window("account-setup");
      await setupWindow.getByRole("textbox", { name: "Your name" }).fill("Bob");
      await setupWindow
        .getByRole("textbox", { name: "Email address" })
        .fill("bob@elsewhere.example");
      await setupWindow.getByRole("button", { name: "Continue" }).click();
      const incoming = setupWindow.getByRole("group", { name: "Incoming mail" });
      await expect(incoming.getByRole("textbox", { name: "Server" })).toBeVisible();
      await sweep.visit("account-setup", "account-setup-servers");
      await expectLinedUpAtWidths(page, "account setup (address step)", () => [
        incoming.getByRole("textbox", { name: "Server" }),
        incoming.getByRole("textbox", { name: "User name" }),
      ]);
      await sweep.close("account-setup");

      await paletteWindowStep(sweep, "snoozed", "Snoozed messages", "snoozed");
      await messageDialogsStep(sweep, server);
      await contextMenusStep(sweep);
      await keyWindowsStep(sweep);
    },
  },
  settings: {
    windows: ["settings"],
    run: async (sweep) => {
      const page = sweep.page;
      for (const name of await settingsSectionNames(page)) {
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
      await settingsDialogsStep(sweep);
    },
  },
  scheduling: {
    windows: ["scheduling-cancel-meeting", "scheduling-find-time", "scheduling-settings"],
    run: async (sweep) => {
      const calendar = await showAgenda(sweep.page);
      await meetingStep(sweep, calendar, { cancel: true });
      await toolbarWindowsStep(sweep, calendarActions(sweep.page), SCHEDULING_ACTION_WINDOWS);
    },
  },
  pim: {
    windows: [
      "pim-event-editor",
      "pim-edit-scope",
      "pim-task-editor",
      "pim-contact-editor",
      "pim-import",
      "pim-sources",
      "pim-contact-groups",
      "pim-merge-contacts",
    ],
    run: async (sweep) => {
      const page = sweep.page;
      const calendar = await showAgenda(page);
      const teamSync = calendar.getByRole("button", { name: /^Team sync,/ }).first();
      await expect(teamSync).toBeVisible();
      await teamSync.click();
      const event = sweep.window("pim-event-editor");
      await sweep.visit("pim-event-editor");
      // ADR 0160: the event's text fields share both edges.
      await expectLinedUpAtWidths(page, "event editor", () => [
        event.getByRole("textbox", { name: "Title", exact: true }),
        event.getByRole("textbox", { name: "Location", exact: true }),
        event.getByRole("textbox", { name: "Description", exact: true }),
      ]);
      await sweep.close("pim-event-editor");
      await meetingStep(sweep, calendar, { editor: "pim-event-editor-meeting", cancel: false });
      await editScopeStep(sweep, teamSync);
      for (const [button, name] of PIM_ACTION_WINDOWS) {
        await toolbarAction(page, calendarActions(page), button);
        // The editors load their item first ("Loading…").
        const ready = { "pim-task-editor": "Title", "pim-contact-editor": "First name" } as const;
        if (name in ready) {
          const label = ready[name as keyof typeof ready];
          await expect(
            sweep.window(name).getByRole("textbox", { name: label, exact: true }),
          ).toBeVisible();
        }
        await sweep.visit(name);
        if (name === "pim-contact-editor") {
          const contact = sweep.window(name);
          await expectLinedUpAtWidths(page, "contact editor", () => [
            contact.getByRole("textbox", { name: "First name", exact: true }),
            contact.getByRole("textbox", { name: "Last name", exact: true }),
            contact.getByRole("textbox", { name: "Shown as", exact: true }),
          ]);
        }
        await sweep.close(name);
      }
      await showView(page, "Contacts");
      const contacts = page.locator('[data-part="view-host.view"][data-window="contacts"]');
      await contacts.getByRole("option", { name: /Grace Hopper/ }).click();
      await toolbarAction(page, contacts, "Groups…");
      await sweep.visit("pim-contact-groups");
      await sweep.close("pim-contact-groups");
      await toolbarAction(page, contacts, "Find duplicates…");
      await sweep.visit("pim-merge-contacts");
      await sweep.close("pim-merge-contacts");
    },
  },
  gmail: {
    windows: ["gmail-label-editor", "gmail-label-delete", "gmail-apply-labels"],
    run: async (sweep) => {
      await syncAccount(sweep.page, "Gmail", 4);
      await gmailLabelsStep(sweep, { what: (state) => `gmail-label-editor-${state}` });
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
  // A remote user, so "Pair a device" can be opened.
  const server = nailPerTest({ seed: "demo-multi" }, async (s) => {
    await createUser(s, "ada");
  });

  test("the groups cover every window of the a11y sweep", async () => {
    const config = await rpcOk(server, { type: "GetConfig" }, "Config");
    const layout = config.layout as { windows: Record<string, unknown> };
    const dialogs = [...FEATURE_DIALOGS];
    expect(dialogs.length, "dialogs of the tour").toBeGreaterThan(30);
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
      await openMail(page, server.url, { rows: DEMO_MULTI_CONVERSATIONS });
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
