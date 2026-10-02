import AxeBuilder from "@axe-core/playwright";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { startNail, type NailServer } from "../support/nail";
import { createUser, rpcOk } from "../support/remote";

/**
 * M2c carry-over (WP65): axe on every window, every settings tab and every
 * dialog of `nail serve --seed demo-multi`, in the default theme, the
 * high-contrast theme and dark mode.
 *
 * The sweep checks its own coverage: every window of the resolved layout,
 * every feature dialog listed in {@link FEATURE_DIALOGS} and every tab of
 * the settings window's `tabs` nodes must have been scanned, so a new
 * window or tab fails here until it is reachable and clean.
 */

const MODES = [
  { name: "default theme", query: "" },
  { name: "high-contrast theme", query: "?mode=high-contrast" },
  { name: "dark mode", query: "?mode=dark" },
] as const;

/**
 * Dialogs features register by name (`window.open "<name>"`) that are not
 * layout windows. M2d's plug-in dialogs (`plugin-install`,
 * `plugin-manager`) join this list with the plug-in integration (WP64).
 */
const FEATURE_DIALOGS = [
  "config-merge",
  "config-errors",
  "plugin-install",
  "remote-pair",
  "security-autocrypt-import",
  "snooze",
  "sync-folder",
  // M3a: calendars, contacts and tasks.
  "pim-event-editor",
  "pim-edit-scope",
  "pim-contact-editor",
  "pim-contact-groups",
  "pim-merge-contacts",
  "pim-task-editor",
  "pim-import",
  "pim-sources",
  // M3b: invitations / scheduling and Gmail labels.
  "scheduling-cancel-meeting",
  "scheduling-settings",
  "scheduling-find-time",
  "gmail-label-editor",
  "gmail-label-delete",
  "gmail-apply-labels",
  // M4a: the confirmation center of AI agents and plug-ins.
  "confirmation-center",
  // M4b: About Nail (says "desktop app only" here; the desktop smoke test
  // shows its facts).
  "about-nail",
  // M4c: the command palette, printing, the import / export / backup /
  // restore wizards and the diagnostics bundle. The recovery-key dialogs
  // are scanned in vault-recovery.spec.ts (they need encrypted data).
  "command-palette",
  "print",
  "import-wizard",
  "export-wizard",
  "backup-wizard",
  "restore-wizard",
  "diagnostics-bundle",
] as const;

/** A layout node as `GetConfig` returns it (only what the sweep reads). */
interface LayoutNode {
  type: string;
  children?: LayoutNode[];
}

/** Number of tabs of all `tabs` nodes under `node`. */
function tabCount(node: LayoutNode): number {
  const own = node.type === "tabs" ? (node.children?.length ?? 0) : 0;
  return own + (node.children ?? []).reduce((sum, child) => sum + tabCount(child), 0);
}

/**
 * Every axe violation, whatever its impact. The composer's preview frame is
 * left out: it shows the outgoing mail in a sandbox without scripts that
 * axe cannot enter (as in a11y.spec.ts).
 */
async function violations(page: Page) {
  const results = await new AxeBuilder({ page })
    .exclude('[data-part="composer.preview.frame"]')
    .analyze();
  return results.violations.map((v) => ({
    id: v.id,
    impact: v.impact,
    help: v.help,
    targets: v.nodes.map((n) => n.target.join(" ")),
  }));
}

class Sweep {
  readonly windows = new Set<string>();
  readonly tabs: string[] = [];

  constructor(
    readonly page: Page,
    readonly mode: string,
  ) {}

  /** Scans the page; failures are collected (soft) with what was open. */
  async scan(what: string) {
    expect.soft(await violations(this.page), `${this.mode}: ${what}`).toEqual([]);
  }

  window(name: string): Locator {
    return this.page.locator(`[data-part="window"][data-window="${name}"]`);
  }

  /** Waits for window `name`, scans it and records it. */
  async scanWindow(name: string, what = name) {
    await expect(this.window(name)).toBeVisible();
    await this.scan(what);
    this.windows.add(name);
  }

  async close(name: string) {
    await this.window(name).locator('[data-part="window.close"]').click();
    await expect(this.window(name)).toBeHidden();
  }

  /** Scans every tab of the first tab list in `container`, recursively. */
  async scanTabs(container: Locator, path: string[] = []) {
    const tablist = container.getByRole("tablist").first();
    const names = await tablist
      .getByRole("tab")
      .evaluateAll((tabs) => tabs.map((tab) => tab.textContent?.trim() ?? ""));
    expect(names.length, `${path.join(" › ")} has tabs`).toBeGreaterThan(0);
    for (const name of names) {
      const tab = tablist.getByRole("tab", { name, exact: true });
      await tab.click();
      await expect(tab).toHaveAttribute("aria-selected", "true");
      const where = [...path, name];
      await this.scan(`settings › ${where.join(" › ")}`);
      this.tabs.push(where.join(" › "));
      const panel = container.getByRole("tabpanel", { name, exact: true });
      if ((await panel.getByRole("tablist").count()) > 0) {
        await this.scanTabs(panel, where);
      }
    }
  }
}

// Fail a missing control fast instead of after the whole test budget.
test.use({ actionTimeout: 20_000 });

for (const mode of MODES) {
  test.describe(`every window, settings tab and dialog (${mode.name})`, () => {
    let server: NailServer;

    test.beforeEach(async () => {
      server = await startNail({ seed: "demo-multi" });
      // A remote user, so "Pair a device" can be opened.
      await createUser(server, "ada");
    });

    test.afterEach(async () => {
      await server.stop();
    });

    test("has no accessibility violations", async ({ page }) => {
      test.setTimeout(300_000);
      const config = await rpcOk(server, { type: "GetConfig" }, "Config");
      const layout = config.layout as {
        windows: Record<string, { root: LayoutNode }[]>;
      };
      const sweep = new Sweep(page, mode.name);

      await page.goto(new URL(mode.query, server.url).toString());
      const rows = page.locator('[data-part="message-list.row"]');
      await expect(rows).toHaveCount(17);
      await sweep.scan("main window");
      sweep.windows.add("main");
      const toolbar = page.getByRole("toolbar", { name: "Main toolbar" });

      // The reader with a message open.
      await rows.filter({ hasText: "Welcome to Nail" }).click();
      await expect(page.locator('[data-part="message-view.header.subject"]')).toContainText(
        "Welcome to Nail",
      );
      await sweep.scan("reader");

      // The composer in each mode (with its security panel).
      await toolbar.getByRole("button", { name: "New message" }).click();
      const composer = sweep.window("composer");
      await sweep.scanWindow("composer", "composer (plain text)");
      await composer.getByRole("radio", { name: "HTML" }).click();
      await expect(composer.locator('[data-part="composer.html.content"]')).toBeVisible();
      await sweep.scan("composer (HTML)");
      await composer.getByRole("radio", { name: "Markdown" }).click();
      await expect(composer.locator('[data-part="composer.markdown.content"]')).toBeVisible();
      await sweep.scan("composer (Markdown)");
      await composer.getByRole("button", { name: "Discard" }).click();
      await expect(composer).toBeHidden();

      // Windows of the main toolbar.
      await toolbar.getByRole("button", { name: "Outbox" }).click();
      await sweep.scanWindow("outbox");
      await sweep.close("outbox");

      await toolbar.getByRole("button", { name: "Filter rules" }).click();
      await sweep.scanWindow("rules", "rules (list)");
      await sweep.window("rules").getByRole("button", { name: "New rule" }).click();
      await expect(sweep.window("rules").getByRole("form", { name: "Rule" })).toBeVisible();
      await sweep.scan("rules (editor)");
      await sweep.close("rules");

      await toolbar.getByRole("button", { name: "Add account" }).click();
      await sweep.scanWindow("account-setup");
      await sweep.close("account-setup");

      await toolbar.getByRole("button", { name: "Snoozed messages" }).click();
      await sweep.scanWindow("snoozed");
      await sweep.close("snoozed");

      // M3a: the calendar, contacts and tasks windows and their dialogs.
      await toolbar.getByRole("button", { name: "Calendar", exact: true }).click();
      const calendar = sweep.window("calendar");
      await expect(calendar.getByRole("button", { name: /^Team sync,/ }).first()).toBeVisible();
      await sweep.scanWindow("calendar", "calendar (week)");
      await calendar.getByRole("button", { name: "Month", exact: true }).click();
      await sweep.scan("calendar (month)");
      await calendar.getByRole("button", { name: "Agenda", exact: true }).click();
      const teamSync = calendar.getByRole("button", { name: /^Team sync,/ }).first();
      await expect(teamSync).toBeVisible();
      await sweep.scan("calendar (agenda)");
      await teamSync.click();
      await sweep.scanWindow("pim-event-editor");
      await sweep.close("pim-event-editor");
      // M3b: a meeting the user organizes (the editor's meeting panel and
      // the cancel dialog).
      await calendar
        .getByRole("button", { name: /^Project kickoff,/ })
        .first()
        .click();
      const meeting = sweep
        .window("pim-event-editor")
        .getByRole("region", { name: "Meeting: Project kickoff" });
      await expect(meeting).toBeVisible();
      await sweep.scan("event editor (meeting)");
      await meeting.getByRole("button", { name: "Cancel meeting…" }).click();
      await sweep.scanWindow("scheduling-cancel-meeting");
      await sweep.close("scheduling-cancel-meeting");
      await sweep.close("pim-event-editor");
      await teamSync.focus();
      await page.keyboard.press("Delete");
      await sweep.scanWindow("pim-edit-scope");
      await sweep.close("pim-edit-scope");
      const calendarActions = page.getByRole("toolbar", { name: "Calendar actions" });
      for (const [button, name] of [
        ["New task", "pim-task-editor"],
        ["New contact", "pim-contact-editor"],
        ["Import calendar or contacts", "pim-import"],
        ["Calendars and address books", "pim-sources"],
        ["Find a time", "scheduling-find-time"],
        ["Invitation settings", "scheduling-settings"],
      ] as const) {
        await calendarActions.getByRole("button", { name: button, exact: true }).click();
        await sweep.scanWindow(name);
        await sweep.close(name);
      }
      await sweep.close("calendar");

      await toolbar.getByRole("button", { name: "Contacts", exact: true }).click();
      const contactsWindow = sweep.window("contacts");
      await contactsWindow.getByRole("option", { name: /Grace Hopper/ }).click();
      await expect(contactsWindow.getByRole("heading", { name: "Grace Hopper" })).toBeVisible();
      await sweep.scanWindow("contacts");
      await contactsWindow.getByRole("button", { name: "Groups…" }).click();
      await sweep.scanWindow("pim-contact-groups");
      await sweep.close("pim-contact-groups");
      await contactsWindow.getByRole("button", { name: "Find duplicates…" }).click();
      await sweep.scanWindow("pim-merge-contacts");
      await sweep.close("pim-merge-contacts");
      await sweep.close("contacts");

      await toolbar.getByRole("button", { name: "Tasks", exact: true }).click();
      await sweep.scanWindow("tasks");
      await sweep.close("tasks");

      // Dialogs of the selected message.
      await rows.filter({ hasText: "Welcome to Nail" }).click();
      const actions = page.getByRole("toolbar", { name: "Message actions" });
      await actions.getByRole("button", { name: "Snooze…" }).click();
      await sweep.scanWindow("snooze");
      await sweep.close("snooze");
      await actions.getByRole("button", { name: "Import Autocrypt setup message" }).click();
      await sweep.scanWindow("security-autocrypt-import");
      await sweep.close("security-autocrypt-import");
      // M4c: printing the selected message.
      await actions.getByRole("button", { name: "Print the message…" }).click();
      await sweep.scanWindow("print");
      await sweep.close("print");

      // Context menus, the move window and a folder's sync dialog.
      await rows.filter({ hasText: "Welcome to Nail" }).click({ button: "right" });
      const menu = page.getByRole("menu");
      await expect(menu).toBeVisible();
      await sweep.scan("message context menu");
      await menu.getByRole("menuitem", { name: "Move to…" }).click();
      await sweep.scanWindow("move");
      await sweep.close("move");
      await page.getByRole("treeitem", { name: /^INBOX/ }).click({ button: "right" });
      await expect(page.getByRole("menu")).toBeVisible();
      await sweep.scan("folder context menu");
      await page.getByRole("menu").getByRole("menuitem", { name: "Synchronization…" }).click();
      await sweep.scanWindow("sync-folder");
      await sweep.close("sync-folder");

      // M4a: the confirmation center (key "g a"; empty here — the cards are
      // scanned in agents.spec.ts).
      await page.keyboard.press("g");
      await page.keyboard.press("a");
      await sweep.scanWindow("confirmation-center");
      await sweep.close("confirmation-center");

      // M4b: About Nail (F1).
      await page.keyboard.press("F1");
      await sweep.scanWindow("about-nail");
      await sweep.close("about-nail");

      // M4c: the command palette (Ctrl+K toggles it).
      await page.keyboard.press("Control+k");
      await sweep.scanWindow("command-palette");
      await page.keyboard.press("Escape");
      await expect(sweep.window("command-palette")).toBeHidden();

      // Every settings tab, nested ones included.
      await toolbar.getByRole("button", { name: "Settings" }).click();
      const settings = sweep.window("settings");
      await expect(settings).toBeVisible();
      sweep.windows.add("settings");
      await sweep.scanTabs(settings);

      // Dialogs opened from settings tabs, stacked over the settings window.
      const top = settings.getByRole("tablist").first();
      await top.getByRole("tab", { name: "Devices", exact: true }).click();
      await settings.getByRole("button", { name: "Pair a device" }).click();
      await sweep.scanWindow("remote-pair");
      await sweep.close("remote-pair");
      await top.getByRole("tab", { name: "Customize", exact: true }).click();
      const customize = page.getByRole("toolbar", { name: "Customization" });
      await customize.getByRole("button", { name: "Apply a theme or layout…" }).click();
      await sweep.scanWindow("config-merge");
      await sweep.close("config-merge");
      await customize.getByRole("button", { name: "Configuration errors" }).click();
      await sweep.scanWindow("config-errors");
      await sweep.close("config-errors");
      // M4c: the wizards and the diagnostics bundle, from their tabs.
      for (const [tab, button, name] of [
        ["Import and export", "Import…", "import-wizard"],
        ["Import and export", "Export…", "export-wizard"],
        ["Backups", "Back up…", "backup-wizard"],
        ["Backups", "Restore…", "restore-wizard"],
        ["Diagnostics", "Create diagnostics bundle…", "diagnostics-bundle"],
      ] as const) {
        await top.getByRole("tab", { name: tab, exact: true }).click();
        await settings.getByRole("button", { name: button }).click();
        await sweep.scanWindow(name);
        await sweep.close(name);
      }
      // The plug-in install dialog (M2d), from the Plug-ins tab.
      await top.getByRole("tab", { name: "Plug-ins", exact: true }).click();
      await settings.getByRole("button", { name: "Install a plug-in…" }).click();
      await sweep.scanWindow("plugin-install");
      await sweep.close("plugin-install");
      // M3b: the demo Gmail account, switched on here (labels, categories,
      // the label dialogs).
      await top.getByRole("tab", { name: "Accounts", exact: true }).click();
      await settings.getByRole("button", { name: /^Gmail/ }).click();
      await settings.getByRole("checkbox", { name: "Synchronize this account" }).check();
      await settings.getByRole("button", { name: "Save", exact: true }).click();
      await expect(settings.getByText("Settings saved.")).toBeVisible();
      await sweep.close("settings");
      const gmail = page
        .locator('[data-part="account-switcher.item"]')
        .filter({ hasText: "Gmail" });
      await expect(gmail).toHaveAttribute("data-status", "online", { timeout: 30_000 });
      await gmail.click();
      await expect(rows).toHaveCount(4, { timeout: 30_000 });
      const labelList = page.locator('[data-part="label-list"]');
      await expect(
        labelList.getByRole("button", { name: "Edit label Work/Projects" }),
      ).toBeVisible();
      await sweep.scan("Gmail account (labels, categories)");
      await labelList.getByRole("button", { name: "New label" }).click();
      await sweep.scanWindow("gmail-label-editor", "gmail-label-editor (new)");
      await sweep.close("gmail-label-editor");
      await labelList.getByRole("button", { name: "Edit label Work/Projects" }).click();
      await sweep.scanWindow("gmail-label-editor", "gmail-label-editor (existing)");
      await sweep.close("gmail-label-editor");
      await labelList.getByRole("button", { name: "Delete label Work/Projects" }).click();
      await sweep.scanWindow("gmail-label-delete");
      await sweep.close("gmail-label-delete");
      await rows.filter({ hasText: "Lunch on Friday?" }).click();
      const invitationReader = page.locator('[data-part="message-view.header.subject"]');
      await expect(invitationReader).toContainText("Lunch on Friday?");
      await sweep.scan("reader (Gmail message with labels)");
      await actions.getByRole("button", { name: "Labels…" }).click();
      await sweep.scanWindow("gmail-apply-labels");
      await sweep.close("gmail-apply-labels");

      // Coverage: every layout window, feature dialog and settings tab.
      const expectedWindows = [...Object.keys(layout.windows), ...FEATURE_DIALOGS].sort();
      expect([...sweep.windows].sort(), "windows and dialogs scanned").toEqual(expectedWindows);
      const settingsRoot = layout.windows.settings?.[0]?.root;
      expect(settingsRoot, "the layout has a settings window").toBeDefined();
      expect(sweep.tabs.length, `settings tabs scanned: ${sweep.tabs.join(", ")}`).toBe(
        tabCount(settingsRoot as LayoutNode),
      );
    });
  });
}
