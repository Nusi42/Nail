import { expect, test, type Page } from "@playwright/test";
import { axeViolations } from "../support/axe";
import { nailPerTest } from "../support/nail";
import { createUser, rpcOk } from "../support/remote";
import {
  newMessageButton,
  openSettings,
  READER_SUBJECT,
  runFromPalette,
  showView,
  syncAccount,
  toolbarAction,
  messageRows,
  DEMO_MULTI_CONVERSATIONS,
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
  toolbarWindowsStep,
} from "../support/windowTour";

/**
 * M2c carry-over (WP65): axe on every window, every settings tab and every
 * dialog of `nail serve --seed demo-multi`, in the default theme, the
 * high-contrast theme and dark mode.
 *
 * The sweep checks its own coverage: every window of the resolved layout
 * (M5b: the views' windows shown in the view host), every feature dialog
 * listed in {@link FEATURE_DIALOGS} and every section of the settings page
 * must have been scanned, so a new window or section fails here until it
 * is reachable and clean.
 */

const MODES = [
  { name: "default theme", query: "" },
  { name: "high-contrast theme", query: "?mode=high-contrast" },
  { name: "dark mode", query: "?mode=dark" },
] as const;

/** A layout node as `GetConfig` returns it (only what the sweep reads). */
interface LayoutNode {
  type: string;
  children?: LayoutNode[];
}

class Sweep extends WindowTour {
  readonly windows = new Set<string>();
  readonly sections: string[] = [];

  constructor(
    page: Page,
    readonly mode: string,
  ) {
    super(page);
  }

  /** Scans the page; failures are collected (soft) with what was open. */
  async scan(what: string) {
    expect.soft(await axeViolations(this.page), `${this.mode}: ${what}`).toEqual([]);
  }

  /** States that are no windows: scanned like them. */
  async state(what: string) {
    await this.scan(what);
  }

  /** Waits for window `name`, scans it and records it. */
  async visit(name: string, what = name) {
    await expect(this.window(name)).toBeVisible();
    await this.scan(what);
    this.windows.add(name);
  }

  /** Scans every section of the settings page (M5b: it holds every former tab). */
  async scanSections() {
    for (const name of await settingsSectionNames(this.page)) {
      await openSettings(this.page, name);
      await this.scan(`settings › ${name}`);
      this.sections.push(name);
    }
  }
}

// Fail a missing control fast instead of after the whole test budget.
test.use({ actionTimeout: 20_000 });

for (const mode of MODES) {
  test.describe(`every window, settings tab and dialog (${mode.name})`, () => {
    // A remote user, so "Pair a device" can be opened.
    const server = nailPerTest({ seed: "demo-multi" }, async (s) => {
      await createUser(s, "ada");
    });

    test("has no accessibility violations", async ({ page }) => {
      test.setTimeout(300_000);
      const config = await rpcOk(server, { type: "GetConfig" }, "Config");
      const layout = config.layout as {
        windows: Record<string, { root: LayoutNode }[]>;
        settings: { sections: unknown[] }[];
      };
      const sweep = new Sweep(page, mode.name);

      await page.goto(new URL(mode.query, server.url).toString());
      const rows = messageRows(page);
      await expect(rows).toHaveCount(DEMO_MULTI_CONVERSATIONS);
      await sweep.scan("main window");
      // The main window (rail + view host) shows the mail view's window.
      sweep.windows.add("main");
      sweep.windows.add("mail");

      // The reader with a message open.
      await rows.filter({ hasText: "Welcome to Nail" }).click();
      await expect(page.locator(READER_SUBJECT)).toContainText("Welcome to Nail");
      await sweep.scan("reader");
      // M5c: "Delete permanently?" is a modal confirmation (cancelled here);
      // P3: it asks only in Trash (a plain-text message: axe waits for HTML
      // mail frames of the reader without end).
      await deletePermanentlyStep(sweep, "Grüße aus München");

      // The composer in each mode (with its security panel).
      await newMessageButton(page).click();
      const composer = sweep.window("composer");
      await sweep.visit("composer", "composer (plain text)");
      await composer.getByRole("radio", { name: "HTML" }).click();
      await expect(composer.locator('[data-part="composer.html.content"]')).toBeVisible();
      await sweep.scan("composer (HTML)");
      await composer.getByRole("radio", { name: "Markdown" }).click();
      await expect(composer.locator('[data-part="composer.markdown.content"]')).toBeVisible();
      await sweep.scan("composer (Markdown)");
      // M5c: discarding a message with changes asks in a modal confirmation.
      await discardComposerStep(sweep);

      // Windows without a button of their own (M5b: the command palette).
      await paletteWindowStep(sweep, "outbox", "Outbox", "outbox");

      await runFromPalette(page, "filter rules", "Filter rules");
      await sweep.visit("rules", "rules (list)");
      await sweep.window("rules").getByRole("button", { name: "New rule" }).click();
      await expect(sweep.window("rules").getByRole("form", { name: "Rule" })).toBeVisible();
      await sweep.scan("rules (editor)");
      await sweep.close("rules");

      await paletteWindowStep(sweep, "add account", "Add account", "account-setup");
      await paletteWindowStep(sweep, "snoozed", "Snoozed messages", "snoozed");

      // M3a: the calendar, contacts and tasks views and their dialogs.
      await showView(page, "Calendar");
      const calendar = sweep.window("calendar");
      await expect(calendar.getByRole("button", { name: /^Team sync,/ }).first()).toBeVisible();
      await sweep.visit("calendar", "calendar (week)");
      await calendar.getByRole("button", { name: "Month", exact: true }).click();
      await sweep.scan("calendar (month)");
      await calendar.getByRole("button", { name: "Agenda", exact: true }).click();
      const teamSync = calendar.getByRole("button", { name: /^Team sync,/ }).first();
      await expect(teamSync).toBeVisible();
      await sweep.scan("calendar (agenda)");
      await teamSync.click();
      await sweep.visit("pim-event-editor");
      await sweep.close("pim-event-editor");
      // M3b: a meeting the user organizes (the editor's meeting panel and
      // the cancel dialog).
      await meetingStep(sweep, calendar, { editor: "event editor (meeting)", cancel: true });
      await editScopeStep(sweep, teamSync);
      await toolbarWindowsStep(sweep, calendarActions(page), [
        ...PIM_ACTION_WINDOWS,
        ...SCHEDULING_ACTION_WINDOWS,
      ]);

      await showView(page, "Contacts");
      const contactsWindow = sweep.window("contacts");
      await contactsWindow.getByRole("option", { name: /Grace Hopper/ }).click();
      await expect(contactsWindow.getByRole("heading", { name: "Grace Hopper" })).toBeVisible();
      await sweep.visit("contacts");
      await toolbarAction(page, contactsWindow, "Groups…");
      await sweep.visit("pim-contact-groups");
      await sweep.close("pim-contact-groups");
      await toolbarAction(page, contactsWindow, "Find duplicates…");
      await sweep.visit("pim-merge-contacts");
      await sweep.close("pim-merge-contacts");

      await showView(page, "Tasks");
      await sweep.visit("tasks");

      // Dialogs of the selected message.
      await showView(page, "Mail");
      await messageDialogsStep(sweep, server);
      // Context menus, the move window and a folder's sync dialog.
      await contextMenusStep(sweep);
      // The confirmation center (empty here — the cards are scanned in
      // agents.spec.ts), About Nail and the command palette.
      await keyWindowsStep(sweep);

      // Every settings section of the settings view.
      sweep.windows.add("settings");
      await sweep.scanSections();
      const settings = sweep.window("settings");

      // Dialogs opened from settings sections.
      await settingsDialogsStep(sweep);
      // M5d: the toolbar customizer (right-click a toolbar).
      await showView(page, "Mail");
      await rows.filter({ hasText: "Welcome to Nail" }).click();
      await page
        .getByRole("toolbar", { name: "Message actions" })
        .getByRole("button", { name: "Reply", exact: true })
        .click({ button: "right" });
      await expect(page.getByRole("menu", { name: "Toolbar" })).toBeVisible();
      await sweep.scan("toolbar context menu");
      await page.getByRole("menuitem", { name: "Customize toolbar" }).click();
      await expect(
        sweep.window("toolbar-customizer").getByRole("listbox", { name: "Available actions" }),
      ).toBeVisible();
      await sweep.visit("toolbar-customizer");
      await sweep.close("toolbar-customizer");
      // M5d: customize-layout mode from the rail's View menu, also while a
      // pane is picked up (the drop targets).
      await page
        .getByRole("navigation", { name: "Sections", exact: true })
        .click({ button: "right", position: { x: 20, y: 300 } });
      const viewMenu = page.getByRole("menu", { name: "View" });
      await expect(viewMenu).toBeVisible();
      await sweep.scan("View menu (navigation rail)");
      await viewMenu.getByRole("menuitem", { name: "Customize layout" }).click();
      const layoutEditor = page.getByRole("region", { name: "Customizing layout: Mail" });
      await expect(layoutEditor).toBeVisible();
      await sweep.scan("customize layout");
      await layoutEditor.getByRole("button", { name: "Move Folders", exact: true }).click();
      await expect(
        page.getByRole("button", { name: "Swap Folders with Message", exact: true }),
      ).toBeVisible();
      await sweep.scan("customize layout (moving a pane)");
      await page.keyboard.press("Escape");
      await layoutEditor.getByRole("button", { name: "Done" }).click();
      await expect(layoutEditor).toBeHidden();
      // M5d: New theme, the theme editor, Rename and Delete (last: it adds a
      // user theme; it is not put to use).
      await openSettings(page, "Appearance");
      await settings.getByRole("button", { name: "New theme" }).click();
      await sweep.visit("theme-new");
      const newTheme = sweep.window("theme-new");
      await newTheme.getByRole("textbox", { name: "Name" }).fill("Sweep");
      await newTheme.getByRole("checkbox", { name: "Use the new theme now" }).uncheck();
      await newTheme.getByRole("button", { name: "Create" }).click();
      const themeEditor = sweep.window("theme-editor");
      await expect(themeEditor.getByLabel("Theme", { exact: true })).toHaveValue("sweep");
      await sweep.visit("theme-editor", "theme editor (colours)");
      await themeEditor.getByRole("tab", { name: "Parts" }).click();
      await sweep.scan("theme editor (parts)");
      await themeEditor.getByRole("button", { name: "Rename" }).click();
      await sweep.visit("theme-rename");
      await sweep.close("theme-rename");
      await themeEditor.getByRole("button", { name: "Delete" }).click();
      await sweep.visit("theme-delete");
      await sweep.close("theme-delete");
      await sweep.close("theme-editor");

      // M3b: the demo Gmail account, switched on here (labels, categories,
      // the label dialogs).
      await syncAccount(page, "Gmail", 4);
      await gmailLabelsStep(sweep, { what: (state) => `gmail-label-editor (${state})` });
      // Coverage: every layout window, feature dialog and settings section.
      const expectedWindows = [...Object.keys(layout.windows), ...FEATURE_DIALOGS].sort();
      expect([...sweep.windows].sort(), "windows and dialogs scanned").toEqual(expectedWindows);
      expect(sweep.sections.length, `settings sections scanned: ${sweep.sections.join(", ")}`).toBe(
        layout.settings.reduce((sum, group) => sum + group.sections.length, 0),
      );
    });
  });
}
