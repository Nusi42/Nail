import {
  copyFileSync,
  mkdirSync,
  readdirSync,
  readFileSync,
  statSync,
  writeFileSync,
} from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { tempDir, repoRoot, type NailServer, nailOnDemand } from "../support/nail";
import { rpcOk } from "../support/remote";
import {
  accountSectionNamed,
  closeWindow,
  messageRows,
  openMail,
  openSettingsTab,
  showView,
  viewRegion,
  windowById,
} from "../support/views";
import { defined } from "../support/defined";

/**
 * M4c WP125 (ADRs 0094, 0096, 0099) through the web UI of `nail serve
 * --seed demo`: importing an mbox and a Maildir with flags, a Thunderbird
 * profile (accounts offered, local folders and address books imported),
 * exporting a folder to mbox and importing it again identically, a backup
 * restored into a wiped Nail, and the command palette running an action.
 * Paths are typed (the web UI has no file dialog).
 */

const MBOX = [
  "From alice@example.org Mon Sep 28 10:00:00 2026",
  "From: Alice <alice@example.org>",
  "To: Ada <ada@example.net>",
  "Subject: Read and flagged",
  "Message-ID: <e2e-first@example.org>",
  "Date: Mon, 28 Sep 2026 10:00:00 +0000",
  "Status: RO",
  "X-Status: F",
  "",
  "Hello.",
  ">From the archive.",
  "",
  "From bob@example.org Tue Sep 29 11:00:00 2026",
  "From: Bob <bob@example.org>",
  "To: Ada <ada@example.net>",
  "Subject: Still unread",
  "Message-ID: <e2e-second@example.org>",
  "Date: Tue, 29 Sep 2026 11:00:00 +0000",
  "Status: O",
  "",
  "Unread.",
  "",
].join("\n");

function writeMbox(dir: string): string {
  const file = path.join(dir, "old-mail.mbox");
  writeFileSync(file, MBOX);
  return file;
}

function writeMaildir(dir: string): string {
  const root = path.join(dir, "Maildir");
  for (const sub of ["cur", "new", "tmp", ".Lists/cur", ".Lists/new", ".Lists/tmp"]) {
    mkdirSync(path.join(root, sub), { recursive: true });
  }
  const message = (id: string, subject: string) =>
    `From: Carol <carol@example.org>\r\nSubject: ${subject}\r\nMessage-ID: <${id}@example.org>\r\nDate: Wed, 30 Sep 2026 09:00:00 +0000\r\n\r\nBody of ${subject}.\r\n`;
  // `!` instead of `:` before the flags: Windows file names cannot hold `:`.
  writeFileSync(
    path.join(root, "cur", "1790000000.1.host!2,FS"),
    message("md1", "Maildir flagged"),
  );
  writeFileSync(path.join(root, "new", "1790000001.2.host"), message("md2", "Maildir new"));
  writeFileSync(
    path.join(root, ".Lists", "cur", "1790000002.3.host!2,RS"),
    message("md3", "Maildir answered"),
  );
  return root;
}

function copyDir(from: string, to: string) {
  mkdirSync(to, { recursive: true });
  for (const entry of readdirSync(from)) {
    const source = path.join(from, entry);
    const target = path.join(to, entry);
    if (statSync(source).isDirectory()) copyDir(source, target);
    else copyFileSync(source, target);
  }
}

interface Folder {
  id: string;
  path: string;
  name: string;
}

interface Summary {
  id: string;
  subject: string;
  flags: { seen: boolean; flagged: boolean; answered: boolean };
}

async function localFolders(server: NailServer): Promise<Folder[]> {
  const answer = await rpcOk(
    server,
    { type: "ListFolders", account_id: "local-folders" },
    "Folders",
  );
  return answer.folders as Folder[];
}

async function messagesIn(server: NailServer, folderId: string): Promise<Summary[]> {
  const answer = await rpcOk(
    server,
    { type: "ListMessages", folder_id: folderId, offset: 0, limit: 100 },
    "Messages",
  );
  return answer.messages as Summary[];
}

const openApp = (page: Page, server: NailServer) => openMail(page, server.url);

/** The settings view at the section of a former settings tab (M5b). */
function settingsTab(page: Page, tab: string): Promise<Locator> {
  return openSettingsTab(page, tab);
}

/** Runs the import wizard on a typed path; answers the dialog. */
async function importWith(
  page: Page,
  format: string,
  fieldLabel: "File" | "Folder",
  source: string,
  folderName?: string,
): Promise<Locator> {
  const settings = await settingsTab(page, "Import and export");
  await settings.getByRole("button", { name: "Import…" }).click();
  const dialog = page.getByRole("dialog", { name: "Import mail, calendars and contacts" });
  await expect(dialog).toBeVisible();
  await dialog.getByRole("radio", { name: format }).check();
  await dialog.getByLabel(fieldLabel, { exact: true }).fill(source);
  await dialog.getByRole("button", { name: "Read source" }).click();
  await expect(dialog.getByText("Step 2 of 3")).toBeVisible({ timeout: 30_000 });
  if (folderName !== undefined) {
    await dialog.getByLabel("New folder for the import").fill(folderName);
  }
  await dialog.getByRole("button", { name: "Import", exact: true }).click();
  await expect(dialog.getByText(/^Import: Done/)).toBeVisible({ timeout: 60_000 });
  return dialog;
}

const server = nailOnDemand();

test("an mbox and a Maildir are imported with their flags", async ({ page }) => {
  await server.start();
  const files = tempDir("import");
  await openApp(page, server);
  const dialog = await importWith(page, "mbox file", "File", writeMbox(files), "Old mail");
  await expect(dialog.getByText("2 messages")).toBeVisible();
  await closeWindow(windowById(page, "import-wizard"));
  await importWith(page, "Maildir folder", "Folder", writeMaildir(files), "Server copy");
  await closeWindow(windowById(page, "import-wizard"));
  await showView(page, "Mail");

  const folders = await localFolders(server);
  const mbox = folders.find((f) => f.path === "Old mail/old-mail");
  expect(mbox, JSON.stringify(folders)).toBeDefined();
  const imported = await messagesIn(server, defined(mbox, "the mbox folder").id);
  expect(imported.find((m) => m.subject === "Read and flagged")?.flags).toMatchObject({
    seen: true,
    flagged: true,
  });
  expect(imported.find((m) => m.subject === "Still unread")?.flags.seen).toBe(false);
  const lists = folders.find((f) => f.path.startsWith("Server copy/") && f.name === "Lists");
  expect(lists).toBeDefined();
  const listMail = await messagesIn(server, defined(lists, "the Lists folder").id);
  expect(listMail[0]?.flags).toMatchObject({ seen: true, answered: true });

  // The imported mail is in the UI: the Local Folders account and its rows.
  // M5b: the Local Folders account's section in the folder pane.
  const account = accountSectionNamed(page, "Local Folders");
  await expect(account).toBeVisible();
  await account.getByRole("treeitem", { name: /old-mail/ }).click();
  const rows = messageRows(page);
  await expect(rows).toHaveCount(2);
  await expect(rows.filter({ hasText: "Read and flagged" })).toHaveAttribute(
    "data-state",
    /flagged/,
  );
  await expect(rows.filter({ hasText: "Still unread" })).toHaveAttribute("data-state", /unread/);
});

test("a Thunderbird profile offers its accounts and imports folders and address books", async ({
  page,
}) => {
  await server.start();
  const profile = path.join(tempDir("thunderbird"), "x7k2m9qa.default-release");
  copyDir(
    path.join(
      repoRoot,
      "crates",
      "nail-portability",
      "tests",
      "fixtures",
      "thunderbird",
      "Profiles",
      "x7k2m9qa.default-release",
    ),
    profile,
  );
  await openApp(page, server);
  const dialog = await importWith(page, "Thunderbird profile", "Folder", profile, "Thunderbird");
  await expect(dialog.getByText("4 messages")).toBeVisible();
  await expect(dialog.getByText("Deleted in the other program and not imported.")).toBeVisible();
  // Accounts are only offered: added without passwords when chosen.
  const proposals = dialog.getByRole("region", { name: "Accounts found in Thunderbird" });
  await expect(proposals.getByRole("checkbox")).toHaveCount(2);
  await proposals.getByRole("button", { name: "Add chosen accounts" }).click();
  await expect(proposals.getByText("Added. Sign in to start syncing.").first()).toBeVisible();
  const filters = dialog.getByRole("region", { name: "Filters found in Thunderbird" });
  await expect(filters.getByRole("checkbox")).toHaveCount(4);
  await closeWindow(windowById(page, "import-wizard"));
  await showView(page, "Mail");

  const accounts = await rpcOk(server, { type: "GetAccountsConfig" }, "AccountsConfig");
  const config = accounts.config as { accounts: { email: string }[] };
  expect(config.accounts.map((a) => a.email)).toContain("ada@example.net");
  const folders = await localFolders(server);
  expect(folders.some((f) => f.path.startsWith("Thunderbird/"))).toBe(true);
  const contacts = await rpcOk(
    server,
    { type: "ListContacts", address_book_ids: null, offset: 0, limit: 100 },
    "Contacts",
  );
  expect((contacts.contacts as unknown[]).length).toBeGreaterThanOrEqual(4);
});

test("a folder exported to mbox imports again identically", async ({ page }) => {
  await server.start();
  const files = tempDir("export");
  await openApp(page, server);
  await importWith(page, "mbox file", "File", writeMbox(files), "Source");
  await closeWindow(windowById(page, "import-wizard"));
  await showView(page, "Mail");

  // Export the imported folder (selected in the folder tree).
  await accountSectionNamed(page, "Local Folders")
    .getByRole("treeitem", { name: /old-mail/ })
    .click();
  const first = path.join(files, "first.mbox");
  const settings = await settingsTab(page, "Import and export");
  await settings.getByRole("button", { name: "Export…" }).click();
  const dialog = page.getByRole("dialog", { name: "Export" });
  await expect(dialog.getByRole("checkbox", { name: "Source/old-mail" })).toBeChecked();
  // One folder without subfolders: one mbox file.
  await dialog.getByRole("checkbox", { name: "Include subfolders" }).uncheck();
  await dialog.getByLabel("Save as").fill(first);
  await dialog.getByRole("button", { name: "Export", exact: true }).click();
  await expect(dialog.getByText(/^Export: Done/)).toBeVisible({ timeout: 60_000 });
  await closeWindow(windowById(page, "export-wizard"));
  await showView(page, "Mail");

  // Import the file again and export the new folder.
  await importWith(page, "mbox file", "File", first, "Again");
  await closeWindow(windowById(page, "import-wizard"));
  await showView(page, "Mail");
  const again = (await localFolders(server)).find((f) => f.path === "Again/first");
  expect(again).toBeDefined();
  const second = path.join(files, "second.mbox");
  await rpcOk(
    server,
    {
      type: "StartExport",
      source: {
        type: "folders",
        folder_ids: [defined(again, "the folder").id],
        include_subfolders: false,
      },
      format: "mbox",
      path: second,
      options: {},
    },
    "PortabilityJob",
  );
  await expect
    .poll(() => statSync(second, { throwIfNoEntry: false })?.size ?? 0)
    .toBeGreaterThan(0);
  // Same messages, same flags, byte for byte.
  await expect.poll(() => readFileSync(second, "utf8")).toBe(readFileSync(first, "utf8"));
});

test("a backup restores into a wiped Nail", async ({ page }) => {
  await server.start();
  const files = tempDir("backup");
  const archive = path.join(files, "nail.nailbackup");
  const passphrase = "correct horse battery staple";
  await openApp(page, server);
  await importWith(page, "mbox file", "File", writeMbox(files), "Archive");
  await closeWindow(windowById(page, "import-wizard"));
  await showView(page, "Mail");
  let settings = await settingsTab(page, "Backups");
  await settings.getByRole("button", { name: "Back up…" }).click();
  let dialog = page.getByRole("dialog", { name: "Back up Nail" });
  await dialog.getByLabel("Passphrase", { exact: true }).fill(passphrase);
  await dialog.getByLabel("Passphrase again").fill(passphrase);
  await dialog.getByLabel("Backup file").fill(archive);
  await dialog.getByRole("button", { name: "Back up", exact: true }).click();
  await expect(dialog.getByRole("region", { name: "In this backup" })).toBeVisible({
    timeout: 60_000,
  });
  await server.stop();

  // A new, empty Nail.
  await server.start();
  await openApp(page, server);
  settings = await settingsTab(page, "Backups");
  await settings.getByRole("button", { name: "Restore…" }).click();
  dialog = page.getByRole("dialog", { name: "Restore from a backup" });
  await dialog.getByLabel("Backup file").fill(archive);
  await dialog.getByLabel("Passphrase").fill(passphrase);
  await dialog.getByRole("button", { name: "Open" }).click();
  await expect(dialog.getByText("Step 2 of 4")).toBeVisible({ timeout: 30_000 });
  await dialog.getByRole("button", { name: "Show what would change" }).click();
  const plan = dialog.getByRole("table", { name: "Planned changes" });
  await expect(plan.getByRole("row", { name: /Local folders/ })).toBeVisible();
  expect(await localFolders(server).catch(() => [])).toEqual([]);
  await dialog.getByRole("button", { name: "Restore now" }).click();
  await expect(dialog.getByRole("table", { name: "What was restored" })).toBeVisible({
    timeout: 60_000,
  });
  const restored = (await localFolders(server)).find((f) => f.path === "Archive/old-mail");
  expect(restored).toBeDefined();
  const mail = await messagesIn(server, defined(restored, "the restored folder").id);
  expect(mail.map((m) => m.subject).sort()).toEqual(["Read and flagged", "Still unread"]);
});

test("the command palette runs an action and remembers it", async ({ page }) => {
  await server.start();
  await openApp(page, server);
  await page.keyboard.press("Control+k");
  const palette = page.getByRole("dialog", { name: "Command palette" });
  await expect(palette).toBeVisible();
  await palette.getByRole("combobox", { name: "Search actions" }).fill("settings");
  const option = palette
    .getByRole("option")
    .filter({ hasText: /^Settings/ })
    .first();
  await expect(option).toBeVisible();
  await option.click();
  await expect(palette).toBeHidden();
  // M5b: Settings is a view (`settings.open` and `view.settings` both show it).
  await expect(viewRegion(page, "Settings")).toBeVisible();
  const recent = await rpcOk(server, { type: "GetRecentActions" }, "RecentActions");
  expect(["settings.open", "view.settings"]).toContain(
    (recent.recent as { actions: { action_id: string }[] }).actions[0]?.action_id,
  );
});
