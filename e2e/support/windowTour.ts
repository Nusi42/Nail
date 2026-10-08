/**
 * The tour of every window and dialog of `nail serve --seed demo-multi`,
 * shared by the window sweeps (wave Q2, DRY): the accessibility sweep
 * (`a11y-windows.spec.ts`, axe on each) and the responsive sweep
 * (`p1-windows.spec.ts`, every standard size). A sweep is a
 * {@link WindowTour}: it says what it does with each window (`visit`) and
 * with states that are no windows of their own (`state`); the steps below
 * open the windows the same way for both.
 */
import { readFileSync } from "node:fs";
import path from "node:path";
import { expect, type Locator, type Page } from "@playwright/test";
import { openCalendar } from "./calendar";
import { control, repoRoot, type NailServer } from "./nail";
import {
  messageRows,
  openSettings,
  READER_SUBJECT,
  runFromPalette,
  selectInTrash,
  settingsNav,
  showAllInboxes,
  toolbarAction,
} from "./views";

/**
 * Dialogs features register by name (`window.open "<name>"`) that are not
 * layout windows: with the layout's windows, every one a sweep must reach.
 */
export const FEATURE_DIALOGS = [
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
  // M4b: About Nail (says "desktop app only" in the browser).
  "about-nail",
  // M4c: the command palette, printing, the import / export / backup /
  // restore wizards and the diagnostics bundle. The recovery-key dialogs
  // are swept in vault-recovery.spec.ts (they need encrypted data); the
  // smartcard PIN dialog needs a token, which `nail serve` has none of
  // (Vitest axe covers it, ui/src/features/security).
  "command-palette",
  "print",
  "import-wizard",
  "export-wizard",
  "backup-wizard",
  "restore-wizard",
  "diagnostics-bundle",
  // M5c: modal confirmations of the mail view and the composer.
  "mail-delete-permanently",
  "composer-discard",
  // M5d: the toolbar customizer and the theme editor with its dialogs.
  "toolbar-customizer",
  "theme-editor",
  "theme-new",
  "theme-rename",
  "theme-delete",
] as const;

/** An Autocrypt Setup Message (the test corpus's). */
export function autocryptSetupMessage(): string {
  return readFileSync(
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
}

/** A sweep over the tour's windows. */
export abstract class WindowTour {
  constructor(readonly page: Page) {}

  /**
   * A dialog window (also an instance `<name>:<key>`, M5c), or a view's
   * window in the view host (M5b).
   */
  window(name: string): Locator {
    return this.page
      .locator(
        `[data-part="window"][data-window="${name}"], [data-part="window"][data-window^="${name}:"], [data-part="view-host.view"][data-window="${name}"]`,
      )
      .last();
  }

  /** What the sweep does with window `name` (it waits for it first); `what` names the state. */
  abstract visit(name: string, what?: string): Promise<void>;

  /** What the sweep does with a state that is no window of its own (a menu, a mode), if anything. */
  state?(what: string): Promise<void>;

  /** Closes window `name` with its close button. */
  async close(name: string): Promise<void> {
    await this.window(name).locator('[data-part="window.close"]').click();
    await expect(this.window(name)).toBeHidden();
  }

  /** Visits window `name`, then closes it. */
  async visitAndClose(name: string, what = name): Promise<void> {
    await this.visit(name, what);
    await this.close(name);
  }
}

/** "Delete permanently?" of the conversation `subject` (moved to Trash first). */
export async function deletePermanentlyStep(tour: WindowTour, subject: string): Promise<void> {
  await selectInTrash(tour.page, subject);
  await tour.page.keyboard.press("Shift+Delete");
  await tour.visitAndClose("mail-delete-permanently");
  await showAllInboxes(tour.page);
}

/** Discards the open composer (with changes): its modal confirmation. */
export async function discardComposerStep(tour: WindowTour): Promise<void> {
  await tour.window("composer").getByRole("button", { name: "Discard" }).click();
  await tour.visit("composer-discard");
  await tour.window("composer-discard").getByRole("button", { name: "Discard" }).click();
  await expect(tour.page.locator('[data-part="window"][data-window^="composer"]')).toHaveCount(0);
}

/** A window opened from the command palette (`query` → `option`). */
export async function paletteWindowStep(
  tour: WindowTour,
  query: string,
  option: string,
  name: string,
): Promise<void> {
  await runFromPalette(tour.page, query, option);
  await tour.visitAndClose(name);
}

/** The reader's action bar. */
const messageActions = (page: Page) => page.getByRole("toolbar", { name: "Message actions" });

/**
 * The dialogs of a selected message: Snooze, the Autocrypt import (only for
 * setup messages, M5b review: one is delivered) and printing (M4c).
 */
export async function messageDialogsStep(tour: WindowTour, server: NailServer): Promise<void> {
  const page = tour.page;
  const rows = messageRows(page);
  const actions = messageActions(page);
  await rows.filter({ hasText: "Welcome to Nail" }).click();
  await toolbarAction(page, actions, "Snooze…");
  await tour.visitAndClose("snooze");
  await control(server, "imap-deliver", { raw: autocryptSetupMessage() });
  const setup = rows.filter({ hasText: "Autocrypt Setup Message" }).first();
  await expect(setup).toBeVisible({ timeout: 20_000 });
  await setup.click();
  await toolbarAction(page, actions, "Import Autocrypt setup message");
  await tour.visitAndClose("security-autocrypt-import");
  await toolbarAction(page, actions, "Print the message…");
  await tour.visitAndClose("print");
}

/** The context menus of a message and a folder: the move window and the folder's sync dialog. */
export async function contextMenusStep(tour: WindowTour): Promise<void> {
  const page = tour.page;
  await messageRows(page).filter({ hasText: "Welcome to Nail" }).click({ button: "right" });
  const menu = page.getByRole("menu");
  await expect(menu).toBeVisible();
  await tour.state?.("message context menu");
  await menu.getByRole("menuitem", { name: "Move to…" }).click();
  await tour.visitAndClose("move");
  await page
    .getByRole("treeitem", { name: /^Inbox/ })
    .first()
    .click({ button: "right" });
  await expect(page.getByRole("menu")).toBeVisible();
  await tour.state?.("folder context menu");
  await page.getByRole("menu").getByRole("menuitem", { name: "Synchronization…" }).click();
  await tour.visitAndClose("sync-folder");
}

/**
 * Windows opened by keys: the confirmation center (M4a, "g a"; empty here),
 * About Nail (M4b, F1) and the command palette (M4c, Ctrl+K toggles it).
 */
export async function keyWindowsStep(tour: WindowTour): Promise<void> {
  const page = tour.page;
  await page.keyboard.press("g");
  await page.keyboard.press("a");
  await tour.visitAndClose("confirmation-center");
  await page.keyboard.press("F1");
  await tour.visitAndClose("about-nail");
  await page.keyboard.press("Control+k");
  await tour.visit("command-palette");
  await page.keyboard.press("Escape");
  await expect(tour.window("command-palette")).toBeHidden();
}

/** The names of the settings page's sections, in navigation order (M5b). */
export async function settingsSectionNames(page: Page): Promise<string[]> {
  const view = await openSettings(page);
  const names = await settingsNav(view)
    .getByRole("button")
    .evaluateAll((items) => items.map((item) => item.textContent.trim()));
  expect(names.length, "settings sections").toBeGreaterThan(0);
  return names;
}

/**
 * Dialogs opened from settings sections: pairing a device (needs a remote
 * user), applying a theme or layout, the configuration errors, the M4c
 * wizards and the diagnostics bundle, the plug-in install dialog (M2d).
 */
export async function settingsDialogsStep(tour: WindowTour): Promise<void> {
  const page = tour.page;
  const settings = tour.window("settings");
  await openSettings(page, "Remote access");
  await settings.getByRole("button", { name: "Pair a device" }).first().click();
  await tour.visitAndClose("remote-pair");
  await openSettings(page, "Layout & toolbars");
  const customize = page.getByRole("toolbar", { name: "Customization" });
  await customize.getByRole("button", { name: "Apply a theme or layout…" }).click();
  await tour.visitAndClose("config-merge");
  await customize.getByRole("button", { name: "Configuration errors" }).click();
  await tour.visitAndClose("config-errors");
  await openSettings(page, "Data & backup");
  for (const [button, name] of [
    ["Import…", "import-wizard"],
    ["Export…", "export-wizard"],
    ["Back up…", "backup-wizard"],
    ["Restore…", "restore-wizard"],
    ["Create diagnostics bundle…", "diagnostics-bundle"],
  ] as const) {
    await settings.getByRole("button", { name: button }).click();
    await tour.visitAndClose(name);
  }
  await openSettings(page, "Plug-ins");
  await settings.getByRole("button", { name: "Install a plug-in…" }).click();
  await tour.visitAndClose("plugin-install");
}

/**
 * M3b: a meeting the user organizes in the `calendar` view (agenda): the
 * event editor with its meeting panel (visited as `editor`, when given)
 * and, with `cancel`, the cancel dialog.
 */
export async function meetingStep(
  tour: WindowTour,
  calendar: Locator,
  options: { editor?: string; cancel: boolean },
): Promise<void> {
  await calendar
    .getByRole("button", { name: /^Project kickoff,/ })
    .first()
    .click();
  const meeting = tour
    .window("pim-event-editor")
    .getByRole("region", { name: "Meeting: Project kickoff" });
  await expect(meeting).toBeVisible();
  if (options.editor !== undefined) await tour.visit("pim-event-editor", options.editor);
  if (options.cancel) {
    await meeting.getByRole("button", { name: "Cancel meeting…" }).click();
    await tour.visitAndClose("scheduling-cancel-meeting");
  }
  await tour.close("pim-event-editor");
}

/** Deleting a recurring event asks which occurrences (`teamSync`: an occurrence). */
export async function editScopeStep(tour: WindowTour, occurrence: Locator): Promise<void> {
  await occurrence.focus();
  await tour.page.keyboard.press("Delete");
  await tour.visitAndClose("pim-edit-scope");
}

/** The Gmail account's label dialogs (its inbox shown): new, edit, delete, apply. */
export async function gmailLabelsStep(
  tour: WindowTour,
  labels: { what: (state: "new" | "existing") => string },
): Promise<void> {
  const page = tour.page;
  const labelList = page.locator('[data-part="label-list"]');
  await expect(labelList.getByRole("button", { name: "Edit label Work/Projects" })).toBeVisible();
  await tour.state?.("Gmail account (labels, categories)");
  await labelList.getByRole("button", { name: "New label" }).click();
  await tour.visitAndClose("gmail-label-editor", labels.what("new"));
  await labelList.getByRole("button", { name: "Edit label Work/Projects" }).click();
  await tour.visitAndClose("gmail-label-editor", labels.what("existing"));
  await labelList.getByRole("button", { name: "Delete label Work/Projects" }).click();
  await tour.visitAndClose("gmail-label-delete");
  await messageRows(page).filter({ hasText: "Lunch on Friday?" }).click();
  await expect(page.locator(READER_SUBJECT)).toContainText("Lunch on Friday?");
  await tour.state?.("reader (Gmail message with labels)");
  await toolbarAction(page, messageActions(page), "Labels…");
  await tour.visitAndClose("gmail-apply-labels");
}

/** The calendar view's toolbar. */
export const calendarActions = (page: Page) =>
  page.getByRole("toolbar", { name: "Calendar actions" });

/** The PIM windows the calendar toolbar opens: [button, window]. */
export const PIM_ACTION_WINDOWS = [
  ["New task", "pim-task-editor"],
  ["New contact", "pim-contact-editor"],
  ["Import calendar or contacts", "pim-import"],
  ["Calendars and address books", "pim-sources"],
] as const;

/** The scheduling windows the calendar toolbar opens: [button, window]. */
export const SCHEDULING_ACTION_WINDOWS = [
  ["Find a time", "scheduling-find-time"],
  ["Invitation settings", "scheduling-settings"],
] as const;

/** Shows the calendar view's agenda; answers the view. */
export const showAgenda = (page: Page) => openCalendar(page, { agenda: true });

/** Visits (and closes) each window a toolbar button opens: [button, window]. */
export async function toolbarWindowsStep(
  tour: WindowTour,
  toolbar: Locator,
  windows: readonly (readonly [string, string])[],
): Promise<void> {
  for (const [button, name] of windows) {
    await toolbarAction(tour.page, toolbar, button);
    await tour.visitAndClose(name);
  }
}
