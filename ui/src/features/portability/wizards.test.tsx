/**
 * The import / export / backup / restore wizards, the job lists and the
 * diagnostics bundle against the mock core's simulated jobs: progress from
 * `PortabilityJobProgress` events, reports with per-item issues,
 * Thunderbird proposals saved only on request, the restore dry run, the
 * exact diagnostics preview, typed paths without a desktop host, and axe.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import axe from "axe-core";
import { describe, expect, it } from "vitest";
import { useActions } from "../../actions/ActionsProvider";
import { NodeView } from "../../layout/LayoutRenderer";
import { WindowLayer } from "../../layout/WindowLayer";
import { createEnv, renderWithProviders, widget, type Env } from "../../test/harness";
import { FirstRun } from "../system/FirstRun";
import {
  BACKUP_DIALOG,
  DIAGNOSTICS_ACTION,
  EXPORT_DIALOG,
  IMPORT_ACTION,
  IMPORT_DIALOG,
  RESTORE_DIALOG,
  advancePortabilityJobs,
  finishPortabilityJobs,
  handlePortabilityEvent,
} from ".";
import { defaultBackupContents } from "./model";

async function violations(root: Element): Promise<string[]> {
  const result = await axe.run(root, {
    rules: { "color-contrast": { enabled: false }, region: { enabled: false } },
  });
  return result.violations.map((v) => `${v.id}: ${v.help} — ${v.nodes[0]?.target.join(" ")}`);
}

/** Routes the core's events to the feature, like the app shell. */
function listen(env: Env): () => void {
  return env.client.subscribe((event) =>
    handlePortabilityEvent(event, {
      client: env.client,
      queryClient: env.queryClient,
      store: env.store,
    }),
  );
}

function setup(options: { desktop?: boolean } = {}) {
  const env = createEnv({ state: { activeAccountId: "acc-personal" } });
  env.transport.data.features.portability.autoProgress = false;
  env.transport.data.features.portability.desktop = options.desktop ?? false;
  const stop = listen(env);
  return { env, stop };
}

function Run({ id }: { id: string }) {
  const actions = useActions();
  return (
    <button type="button" onClick={() => void actions.dispatch(id)}>
      {`run ${id}`}
    </button>
  );
}

function renderDialogs(env: Env, extra: React.ReactNode = null) {
  return renderWithProviders(
    <>
      {extra}
      <WindowLayer layout={env.transport.data.layout} />
    </>,
    env,
  );
}

function step(env: Env, n = 1) {
  act(() => advancePortabilityJobs(env.transport, n));
}

function finish(env: Env) {
  act(() => finishPortabilityJobs(env.transport));
}

describe("import wizard", () => {
  it("imports an mbox file with progress and a report of every problem", async () => {
    const { env, stop } = setup();
    const user = userEvent.setup();
    const { container } = renderDialogs(env, <Run id={IMPORT_ACTION} />);
    await user.click(screen.getByRole("button", { name: `run ${IMPORT_ACTION}` }));
    const dialog = await screen.findByRole("dialog", {
      name: "Import mail, calendars and contacts",
    });
    expect(within(dialog).getByText("Step 1 of 3")).toBeVisible();
    expect(within(dialog).getByRole("radio", { name: "mbox file" })).toBeChecked();
    await user.type(within(dialog).getByLabelText("File"), "archive.mbox");
    expect(within(dialog).getByText(/Type the full path, for example/)).toBeVisible();
    expect(within(dialog).getByRole("button", { name: "Read source" })).toBeDisabled();
    await user.clear(within(dialog).getByLabelText("File"));
    await user.type(within(dialog).getByLabelText("File"), "C:\\Mail\\archive.mbox");
    await user.click(within(dialog).getByRole("button", { name: "Read source" }));
    expect(await within(dialog).findByText("Step 2 of 3")).toBeVisible();
    expect(within(dialog).getByRole("checkbox", { name: "archive" })).toBeChecked();
    expect(within(dialog).getByText("120 messages")).toBeVisible();
    expect(within(dialog).getByText("The last message of the file is cut off.")).toBeVisible();
    expect(within(dialog).getByLabelText("New folder for the import")).toHaveValue(
      "Import from archive.mbox",
    );
    expect(await violations(container)).toEqual([]);
    await user.click(within(dialog).getByRole("button", { name: "Import" }));
    expect(env.transport.requests.find((r) => r.type === "StartImport")).toMatchObject({
      options: {
        mail: { account_id: null, folder_name: "Import from archive.mbox" },
        folders: null,
        skip_duplicates: true,
        keep_flags: true,
      },
    });
    step(env, 3);
    expect(await within(dialog).findByText("Import: Reading")).toBeVisible();
    expect(within(dialog).getByRole("progressbar", { name: "Progress" })).toBeVisible();
    expect(within(dialog).getByText(/^[0-9]+ of 118$/)).toBeVisible();
    expect(await violations(container)).toEqual([]);
    finish(env);
    expect(await within(dialog).findByText("Import: Done, with problems")).toBeVisible();
    expect(within(dialog).getByText("118 messages")).toBeVisible();
    expect(within(dialog).getByText("1 already there (left out)")).toBeVisible();
    const issues = within(dialog).getByRole("region", { name: "Problems" });
    expect(within(issues).getByText("This is not a valid message.")).toBeVisible();
    expect(within(issues).getByText("item 17")).toBeVisible();
    expect(within(issues).getByText("Welcome to the list")).toBeVisible();
    expect(within(issues).getAllByText("Details").length).toBeGreaterThan(0);
    expect(await violations(container)).toEqual([]);
    stop();
  });

  it("imports a Thunderbird profile and adds only the chosen proposals", async () => {
    const { env, stop } = setup({ desktop: true });
    const user = userEvent.setup();
    renderDialogs(env);
    act(() => env.store.getState().openWindow(IMPORT_DIALOG));
    const dialog = await screen.findByRole("dialog");
    await user.click(await within(dialog).findByRole("button", { name: "Use this profile" }));
    expect(await within(dialog).findByText("Step 2 of 3")).toBeVisible();
    expect(within(dialog).getByRole("checkbox", { name: "Personal Address Book" })).toBeChecked();
    expect(
      within(dialog).getByText(/2 accounts and 2 filters are offered after the import/),
    ).toBeVisible();
    await user.click(within(dialog).getByRole("checkbox", { name: "Collected Addresses" }));
    await user.click(within(dialog).getByRole("button", { name: "Import" }));
    expect(env.transport.requests.find((r) => r.type === "StartImport")).toMatchObject({
      source: { format: "thunderbird-profile" },
      options: { collections: ["jsaddrbook://abook.sqlite"] },
    });
    finish(env);
    const proposals = await within(dialog).findByRole("region", {
      name: "Accounts found in Thunderbird",
    });
    // The account already in Nail is not chosen by default.
    expect(within(proposals).getByRole("checkbox", { name: /ada@example\.org/ })).not.toBeChecked();
    expect(within(proposals).getByText("Already in Nail as “acc-personal”.")).toBeVisible();
    await user.click(within(proposals).getByRole("button", { name: "Add chosen accounts" }));
    await waitFor(() =>
      expect(env.transport.requests.filter((r) => r.type === "AddAccount")).toHaveLength(1),
    );
    expect(env.transport.requests.find((r) => r.type === "AddAccount")).toMatchObject({
      config: { email: "ada@club.example" },
      secrets: [],
    });
    expect(await within(proposals).findByText("Added. Sign in to start syncing.")).toBeVisible();
    const rules = within(dialog).getByRole("region", { name: "Filters found in Thunderbird" });
    await user.click(within(rules).getByRole("button", { name: "Save chosen filters as rules" }));
    await waitFor(() =>
      expect(env.transport.requests.filter((r) => r.type === "SaveRule")).toHaveLength(2),
    );
    const saved = env.transport.requests.filter((r) => r.type === "SaveRule");
    expect(saved.map((r) => (r.type === "SaveRule" ? [r.rule.name, r.rule.enabled] : []))).toEqual([
      ["Newsletters", true],
      ["Junk by size", false],
    ]);
    stop();
  });

  it("reports what the core refuses and goes back", async () => {
    const { env, stop } = setup();
    env.transport.data.features.portability.missing = ["C:\\gone"];
    const user = userEvent.setup();
    renderDialogs(env);
    act(() => env.store.getState().openWindow(IMPORT_DIALOG));
    const dialog = await screen.findByRole("dialog");
    await user.click(within(dialog).getByRole("radio", { name: "Maildir folder" }));
    await user.type(within(dialog).getByLabelText("Folder"), "C:\\gone");
    await user.click(within(dialog).getByRole("button", { name: "Read source" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "There is no file or folder at this path.",
    );
    await user.clear(within(dialog).getByLabelText("Folder"));
    await user.type(within(dialog).getByLabelText("Folder"), "C:\\Maildir");
    await user.click(within(dialog).getByRole("button", { name: "Read source" }));
    expect(await within(dialog).findByRole("checkbox", { name: "Archive/2024" })).toBeChecked();
    await user.click(within(dialog).getByRole("button", { name: "Back" }));
    expect(within(dialog).getByLabelText("Folder")).toHaveValue("C:\\Maildir");
    stop();
  });

  it("is what the first-run screen's Import button opens", async () => {
    const { env, stop } = setup();
    const user = userEvent.setup();
    renderDialogs(env, <FirstRun close={() => undefined} />);
    await user.click(await screen.findByRole("button", { name: "Import…" }));
    expect(env.store.getState().windows).toContain(IMPORT_DIALOG);
    stop();
  });
});

describe("export wizard", () => {
  it("exports folders and refuses an existing target until Replace is ticked", async () => {
    const { env, stop } = setup();
    env.transport.data.features.portability.existing = ["C:\\out\\inbox.mbox"];
    act(() => env.store.getState().selectFolder("acc-personal/Inbox"));
    const user = userEvent.setup();
    const { container } = renderDialogs(env);
    act(() => env.store.getState().openWindow(EXPORT_DIALOG));
    const dialog = await screen.findByRole("dialog", { name: "Export" });
    expect(await within(dialog).findByRole("checkbox", { name: "Inbox" })).toBeChecked();
    expect(within(dialog).getByRole("radio", { name: "The selected message" })).toBeDisabled();
    await user.click(within(dialog).getByRole("checkbox", { name: "Include subfolders" }));
    expect(within(dialog).queryByRole("radio", { name: "Calendar file (.ics)" })).toBeNull();
    await user.type(within(dialog).getByLabelText("Save as"), "C:\\out\\inbox.mbox");
    expect(await violations(container)).toEqual([]);
    await user.click(within(dialog).getByRole("button", { name: "Export" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(/The file exists/);
    await user.click(within(dialog).getByRole("checkbox", { name: "Replace existing files" }));
    await user.click(within(dialog).getByRole("button", { name: "Export" }));
    finish(env);
    expect(await within(dialog).findByText(/^Export: Done/)).toBeVisible();
    expect(env.transport.requests.filter((r) => r.type === "StartExport").at(-1)).toMatchObject({
      format: "mbox",
      source: { type: "folders", folder_ids: ["acc-personal/Inbox"], include_subfolders: false },
      options: { overwrite: true, mbox_variant: "mboxrd" },
    });
    stop();
  });
});

describe("backup and restore", () => {
  it("backs up with a checked passphrase, then checks the backup", async () => {
    const { env, stop } = setup({ desktop: true });
    const user = userEvent.setup();
    const { container } = renderDialogs(env);
    act(() => env.store.getState().openWindow(BACKUP_DIALOG));
    const dialog = await screen.findByRole("dialog", { name: "Back up Nail" });
    expect(within(dialog).getByRole("checkbox", { name: "Encryption keys" })).not.toBeChecked();
    await user.click(within(dialog).getByRole("checkbox", { name: "Encryption keys" }));
    expect(within(dialog).getByText(/will contain your secret keys/)).toBeVisible();
    await user.type(within(dialog).getByLabelText("Passphrase"), "short");
    expect(
      within(dialog).getByText("The passphrase must have at least 12 characters."),
    ).toBeVisible();
    await user.type(within(dialog).getByLabelText("Passphrase"), " but now long");
    await user.type(within(dialog).getByLabelText("Passphrase again"), "different");
    expect(within(dialog).getByText("The two passphrases are different.")).toBeVisible();
    await user.clear(within(dialog).getByLabelText("Passphrase again"));
    await user.type(within(dialog).getByLabelText("Passphrase again"), "short but now long");
    await user.click(within(dialog).getByRole("button", { name: "Browse…" }));
    expect((within(dialog).getByLabelText("Backup file") as HTMLInputElement).value).toMatch(
      /Nail backup \d{4}-\d{2}-\d{2}\.nailbackup$/,
    );
    expect(await violations(container)).toEqual([]);
    await user.click(within(dialog).getByRole("button", { name: "Back up" }));
    expect(env.transport.requests.find((r) => r.type === "CreateBackup")).toMatchObject({
      contents: { items: [...defaultBackupContents().items, "keys"] },
      passphrase: "short but now long",
    });
    finish(env);
    const manifest = await within(dialog).findByRole("region", { name: "In this backup" });
    expect(within(manifest).getByText(/^Encryption keys: 1 file/)).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Check this backup" }));
    finish(env);
    const check = await within(dialog).findByRole("region", { name: "Backup check" });
    expect(await within(check).findByText("Backup check: Done")).toBeVisible();
    stop();
  });

  it("opens a backup, shows the dry run and restores only after confirming", async () => {
    const { env, stop } = setup();
    const path = "C:\\Backups\\nail.nailbackup";
    await env.transport.send({
      type: "CreateBackup",
      path,
      contents: defaultBackupContents(),
      passphrase: "correct horse battery",
    });
    act(() => finishPortabilityJobs(env.transport));
    env.transport.data.features.portability.backups[path]?.damaged.push("config/layout.kdl");
    const user = userEvent.setup();
    const { container } = renderDialogs(env);
    act(() => env.store.getState().openWindow(RESTORE_DIALOG));
    const dialog = await screen.findByRole("dialog", { name: "Restore from a backup" });
    await user.type(within(dialog).getByLabelText("Backup file"), path);
    await user.type(within(dialog).getByLabelText("Passphrase"), "wrong passphrase");
    await user.click(within(dialog).getByRole("button", { name: "Open" }));
    expect(await within(dialog).findByRole("alert")).toHaveTextContent(
      "The passphrase is wrong, or this is not a Nail backup.",
    );
    await user.clear(within(dialog).getByLabelText("Passphrase"));
    await user.type(within(dialog).getByLabelText("Passphrase"), "correct horse battery");
    await user.click(within(dialog).getByRole("button", { name: "Open" }));
    expect(await within(dialog).findByText("Step 2 of 4")).toBeVisible();
    await user.click(within(dialog).getByRole("button", { name: "Check the backup first" }));
    finish(env);
    expect(await within(dialog).findByText("This part of the backup is damaged.")).toBeVisible();
    await user.click(within(dialog).getByRole("checkbox", { name: "Plug-ins" }));
    await user.click(within(dialog).getByRole("button", { name: "Show what would change" }));
    const plan = await within(dialog).findByRole("table", { name: "Planned changes" });
    expect(within(plan).getByRole("row", { name: /Theme Replace/ })).toBeVisible();
    expect(
      within(plan).getByRole("row", { name: /Local folders Add what is missing/ }),
    ).toBeVisible();
    expect(within(plan).getByRole("row", { name: /Plug-ins Leave as is/ })).toBeVisible();
    expect(within(dialog).getByText(/Enter the passwords of these accounts/)).toBeVisible();
    expect(within(dialog).getByText(/Nothing is deleted/)).toBeVisible();
    expect(env.transport.requests.some((r) => r.type === "StartRestore")).toBe(false);
    expect(await violations(container)).toEqual([]);
    await user.click(within(dialog).getByRole("button", { name: "Restore now" }));
    finish(env);
    expect(await within(dialog).findByRole("table", { name: "What was restored" })).toBeVisible();
    expect(within(dialog).getByText("Restart Nail to finish the restore.")).toBeVisible();
    expect(env.transport.requests.find((r) => r.type === "StartRestore")).toMatchObject({
      contents: { items: expect.not.arrayContaining(["plugins"]) },
    });
    stop();
  });
});

describe("job lists", () => {
  it("list recent jobs with progress and open their report", async () => {
    const { env, stop } = setup();
    await env.transport.send({
      type: "StartImport",
      source: { format: "vcard", path: "C:\\contacts.vcf" },
      options: {
        mail: { account_id: null, parent_folder_id: null, folder_name: null },
        folders: null,
        collections: null,
        calendar_id: null,
        address_book_id: null,
        skip_duplicates: true,
        keep_flags: true,
      },
    });
    const user = userEvent.setup();
    const { container } = renderDialogs(
      env,
      <>
        <NodeView node={widget("import-export")} path="t/0" />
        <NodeView node={widget("backup-restore")} path="t/1" />
        <NodeView node={widget("diagnostics")} path="t/2" />
      </>,
    );
    const row = await screen.findByText("C:\\contacts.vcf");
    expect(row.closest("li")).toHaveAttribute("data-state", "running");
    expect(screen.getByText("Nothing imported, exported or backed up recently.")).toBeVisible();
    expect(await violations(container)).toEqual([]);
    finish(env);
    await waitFor(() => expect(row.closest("li")).toHaveAttribute("data-state", "completed"));
    await user.click(screen.getByRole("button", { name: "Show report" }));
    const dialog = await screen.findByRole("dialog", { name: "Report" });
    expect(await within(dialog).findByText("32 contacts")).toBeVisible();
    await user.click(
      within(dialog).getAllByRole("button", { name: "Close" }).at(-1) as HTMLElement,
    );
    await user.click(screen.getByRole("button", { name: "Import…" }));
    expect(env.store.getState().windows).toContain(IMPORT_DIALOG);
    stop();
  });
});

describe("widget options", () => {
  it("hide the job list with show-jobs=#false", async () => {
    const { env, stop } = setup();
    renderDialogs(
      env,
      <NodeView node={widget("import-export", { options: { "show-jobs": false } })} path="t" />,
    );
    expect(await screen.findByRole("button", { name: "Import…" })).toBeVisible();
    expect(screen.queryByRole("region", { name: "Recent imports and exports" })).toBeNull();
    stop();
  });
});

describe("diagnostics bundle", () => {
  it("shows exactly what goes in before saving, with mail only when picked", async () => {
    const { env, stop } = setup();
    const first = env.transport.data.messages.find((m) => m.folder_id === "acc-personal/Inbox");
    act(() => env.store.getState().selectMessage(first?.id ?? null));
    const user = userEvent.setup();
    const { container } = renderDialogs(env, <Run id={DIAGNOSTICS_ACTION} />);
    await user.click(screen.getByRole("button", { name: `run ${DIAGNOSTICS_ACTION}` }));
    const dialog = await screen.findByRole("dialog", { name: "Diagnostics for support" });
    expect(
      within(dialog).getByRole("checkbox", { name: "Chosen messages (full content)" }),
    ).not.toBeChecked();
    await user.click(within(dialog).getByRole("button", { name: "Show what will be included" }));
    const preview = await within(dialog).findByRole("region", {
      name: "The bundle will contain exactly these files",
    });
    expect(within(preview).getByText("logs/nail.log")).toBeVisible();
    expect(within(preview).getByText("manifest.json")).toBeVisible();
    expect(within(preview).queryByText(/\.eml$/)).toBeNull();
    expect(within(preview).getAllByText("Passwords and keys removed").length).toBeGreaterThan(0);
    expect(await violations(container)).toEqual([]);
    // Opting in to mail needs a message; the preview is asked for again.
    await user.click(
      within(dialog).getByRole("checkbox", { name: "Chosen messages (full content)" }),
    );
    expect(within(dialog).queryByRole("region", { name: /exactly these files/ })).toBeNull();
    expect(within(dialog).getByText("Choose the messages to include.")).toBeVisible();
    expect(
      within(dialog).getByRole("button", { name: "Show what will be included" }),
    ).toBeDisabled();
    await user.click(await within(dialog).findByRole("checkbox", { name: first?.subject ?? "" }));
    await user.click(within(dialog).getByRole("button", { name: "Show what will be included" }));
    expect(await within(dialog).findByText(/\.eml$/)).toBeVisible();
    await user.type(within(dialog).getByLabelText("Save the bundle as"), "C:\\support\\nail.zip");
    await user.click(within(dialog).getByRole("button", { name: "Save bundle" }));
    expect(await within(dialog).findByText(/Checksum \(SHA-256\): 9f2c4e1a/)).toBeVisible();
    expect(env.transport.requests.find((r) => r.type === "CreateDiagnosticsBundle")).toMatchObject({
      options: { items: expect.arrayContaining(["mail-content"]), message_ids: [first?.id] },
      path: "C:\\support\\nail.zip",
    });
    stop();
  });
});
