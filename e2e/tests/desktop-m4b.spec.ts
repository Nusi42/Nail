import { execFileSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test } from "@playwright/test";
import {
  desktopBinary,
  expectWindowsInBackground,
  fakeInstaller,
  mapiDll,
  mapiHarness,
  shellControl,
  skipFirstRun,
  type DesktopOptions,
  desktopTests,
  configWithIdentity,
  desktopPerTest,
} from "../support/desktop";
import { tempDir } from "../support/nail";
import { commandManifest, writePluginFolder, pluginDevConsole } from "../support/plugins";
import { startUpdateServer, updateKey } from "../support/updates";
import { composerWindow, openSettingsTab } from "../support/views";

/**
 * The Windows integration of wave M4b (plan WP115, ADR 0092) on the real
 * desktop app (`nail-app.exe --seed demo`, WebView2 over CDP): mail from
 * another program through `NailMapi.dll` (Simple MAPI) opens the composer
 * with the file attached; the tray's "New message"; starting hidden with
 * Windows; the updater finding a newer version on a loopback update
 * server and installing it on "Restart to install"; an MSI installation
 * (DISABLEUPDATES=1) hiding the updater; `nail plugin dev` loading a
 * plug-in into the desktop app; the first-run screen and the settings.
 *
 * The tray menu itself is an OS menu Playwright cannot click: the shell's
 * seeded-only test control runs the same handler (`shellControl`). The
 * MSI facts are staged below a throw-away HKCU key (`NAIL_REGISTRY_ROOT`);
 * the real silent MSI install runs in CI (`scripts/msi-smoke.ps1`).
 *
 * Needs Windows with the WebView2 runtime and `npm run build:desktop`. In
 * CI it runs only with `NAIL_E2E_DESKTOP=1`; locally it always runs.
 */

desktopTests();

/** An app with its page (the first-run screen still open); stopped after the test. */
const launchApp = desktopPerTest();
const launch = (options: DesktopOptions = {}) => launchApp({ ...options, keepFirstRun: true });

test("the first run, About Nail and the desktop settings", async () => {
  const { page } = await launch();
  // The welcome screen, once per profile until completed.
  const welcome = page.getByRole("dialog", { name: "Welcome to Nail" });
  await expect(welcome).toBeVisible({ timeout: 60_000 });
  await expect(welcome).toContainText("Open e-mail links with Nail");
  await expect(welcome.getByRole("button", { name: "Choose in Default apps" })).toBeVisible();
  await welcome.getByRole("button", { name: "Get started" }).click();
  await expect(welcome).toBeHidden();

  // F1: About Nail with the installation facts (a development build).
  await page.keyboard.press("F1");
  const about = page.getByRole("dialog", { name: "About Nail" });
  await expect(about).toBeVisible();
  await expect(about).toContainText("Development build");
  await expect(about).toContainText("Development builds do not update themselves.");
  await about.getByRole("button", { name: "Close" }).click();

  // Settings: tray and startup (the demo's registry is in memory).
  const settings = await openSettingsTab(page, "Tray and startup");
  const closeToTray = settings.getByRole("checkbox", {
    name: "Keep Nail running in the notification area",
  });
  await expect(closeToTray).toBeChecked();
  const startWithWindows = settings.getByRole("checkbox", { name: "Start Nail with Windows" });
  await expect(startWithWindows).not.toBeChecked();
  // Saved through the core (the switch follows its answer).
  await startWithWindows.click();
  await expect(startWithWindows).toBeChecked();
  await expect(
    settings.getByRole("checkbox", { name: "Start in the notification area, without a window" }),
  ).toBeEnabled();
  // Default apps: what Windows tells, and the button to Settings.
  await openSettingsTab(page, "Default apps");
  await expect(settings.locator('[data-part="default-apps.item"]').first()).toBeVisible();
  await expect(settings.getByRole("button", { name: "Choose in Default apps" })).toBeVisible();
});

test("the MAPI test harness sends a file and the composer opens with it attached", async () => {
  const pipe = `\\\\.\\pipe\\nail-mapi-e2e-${String(process.pid)}-${String(Date.now())}`;
  const { page } = await launch({
    args: ["--config-dir", configWithIdentity()],
    env: { NAIL_MAPI_PIPE: pipe },
  });
  await skipFirstRun(page);
  const files = tempDir("mapi-files");
  const file = path.join(files, "q3-figures.csv");
  writeFileSync(file, "quarter,revenue\nQ3,42\n");

  // A program's "Send to → Mail recipient" without a window of its own
  // (MAPI_DIALOG off): Nail opens the composer anyway and never sends.
  const result = execFileSync(
    mapiHarness(),
    [
      "send",
      "--dll",
      mapiDll(),
      "--no-ui",
      "--to",
      "grace@navy.example",
      "--subject",
      "Quarterly figures",
      "--body",
      "Here are the figures.",
      "--attach",
      `${file}|figures.csv`,
    ],
    { env: { ...process.env, NAIL_MAPI_PIPE: pipe }, encoding: "utf8" },
  );
  expect(result.trim()).toBe("0");
  // The DLL handed Nail a private copy: the program's file is untouched.
  expect(existsSync(file)).toBe(true);

  const dialog = composerWindow(page);
  await expect(dialog).toBeVisible({ timeout: 30_000 });
  await expect(dialog.getByRole("textbox", { name: "Subject" })).toHaveValue("Quarterly figures");
  await expect(dialog.getByRole("textbox", { name: "To" })).toHaveValue(/grace@navy\.example/);
  await expect(dialog.getByRole("textbox", { name: "Message" })).toHaveValue(
    /^Here are the figures\./,
  );
  await expect(dialog.locator('[data-part="composer.attachment.name"]')).toHaveText([
    "figures.csv",
  ]);
  // The note names the program; nothing was sent.
  const note = dialog.getByRole("note");
  await expect(note).toContainText(/nail-mapi-harness(\.exe)? prepared this message/i);
  await note.getByRole("button", { name: "Dismiss" }).click();
  await expect(note).toBeHidden();
});

test("the tray's New message opens the composer, and start hidden keeps the window away", async () => {
  // Started by the Run key: start hidden is on by default, so only the
  // tray icon shows.
  const { app, page } = await launch({ args: ["--autostart"] });
  await expect.poll(() => shellControl(page, "window-visible")).toBe(false);
  // Tray → New message: the window comes up with an empty composer.
  await shellControl(page, "tray-new-message");
  await expect.poll(() => shellControl(page, "window-visible")).toBe(true);
  // Shown for the shell, yet cloaked and not activated (ADR 0146).
  const shown = await expectWindowsInBackground(page);
  expect(shown.windows.find((w) => w.label === "main")?.visible).toBe(true);
  const dialog = composerWindow(page);
  await expect(dialog).toBeVisible({ timeout: 30_000 });
  await expect(dialog.getByRole("textbox", { name: "To" })).toHaveValue("");
  await expect(dialog.getByRole("textbox", { name: "Subject" })).toHaveValue("");
  // Tray → Quit ends Nail (close to tray does not keep it).
  await shellControl(page, "tray-quit").catch(() => undefined);
  await expect.poll(() => app.hasExited(), { timeout: 30_000 }).toBe(true);
});

test("the updater finds a newer version on a loopback server and installs it on restart", async () => {
  const key = updateKey();
  const server = await startUpdateServer({
    installer: fakeInstaller(),
    version: "1.0.1",
    notes: "## What's new\n\n- Faster sync",
    key,
  });
  const record = path.join(tempDir("fake-installer"), "runs.jsonl");
  try {
    const { app, page } = await launch({
      env: {
        NAIL_UPDATE_ENDPOINT: server.endpoint,
        NAIL_UPDATE_PUBKEY: key.publicKey,
        NAIL_FAKE_INSTALLER_RECORD: record,
      },
    });
    await skipFirstRun(page);
    const settings = await openSettingsTab(page, "Updates");
    const status = settings.locator('[data-part="update-status"]');
    // Check now (or the automatic check got there first); the download
    // runs in the background (the default) and the signature is checked.
    const check = settings.getByRole("button", { name: "Check now" });
    if (await check.isVisible()) await check.click();
    await expect(status).toHaveAttribute("data-state", "ready-to-install", { timeout: 60_000 });
    await expect(status).toContainText("Version 1.0.1 is ready.");
    expect(server.requests).toContain("/stable/latest.json");
    expect(existsSync(record)).toBe(false);

    await settings.getByRole("button", { name: "Restart to install" }).click();
    // Nail runs the installer (passively, as an update that restarts
    // Nail) and quits.
    await expect.poll(() => app.hasExited(), { timeout: 30_000 }).toBe(true);
    await expect.poll(() => existsSync(record), { timeout: 30_000 }).toBe(true);
    const runs = readFileSync(record, "utf8")
      .trim()
      .split("\n")
      .map((line) => JSON.parse(line) as string[]);
    expect(runs).toHaveLength(1);
    expect(runs[0]).toEqual(expect.arrayContaining(["/P", "/UPDATE"]));
  } finally {
    await server.close();
  }
});

test("an MSI installation (DISABLEUPDATES=1) hides the updater", async () => {
  // The facts the MSI writes under HKLM\Software\Nail\Install, staged below
  // a throw-away HKCU key the shell reads instead of the real hives.
  const root = `Software\\NailTests\\e2e-msi-${String(process.pid)}-${String(Date.now())}`;
  const install = `HKCU\\${root}\\HKLM\\Software\\Nail\\Install`;
  const reg = (args: string[]) => execFileSync("reg", args, { stdio: "ignore" });
  const exeDir = path.dirname(desktopBinary());
  reg(["add", install, "/v", "Installer", "/d", "msi", "/f"]);
  reg(["add", install, "/v", "Scope", "/d", "per-machine", "/f"]);
  reg(["add", install, "/v", "InstallDir", "/d", exeDir, "/f"]);
  reg(["add", install, "/v", "DisableUpdates", "/t", "REG_DWORD", "/d", "1", "/f"]);
  try {
    const key = updateKey();
    const { page } = await launch({
      // Even with an update source, the MSI's policy wins.
      env: {
        NAIL_REGISTRY_ROOT: root,
        NAIL_UPDATE_ENDPOINT: "http://127.0.0.1:9/{{channel}}/latest.json",
        NAIL_UPDATE_PUBKEY: key.publicKey,
      },
    });
    await skipFirstRun(page);
    const settings = await openSettingsTab(page, "Updates");
    const status = settings.locator('[data-part="update-status"]');
    await expect(status).toHaveAttribute("data-state", "disabled");
    await expect(status).toContainText("Your organization installs updates for Nail.");
    await expect(settings.getByRole("button", { name: "Check now" })).toHaveCount(0);
    await expect(settings.getByRole("button", { name: "Download" })).toHaveCount(0);
    await expect(settings).toContainText("Installed by your organization (MSI)");
  } finally {
    reg(["delete", `HKCU\\${root}`, "/f"]);
  }
});

test("nail plugin dev loads a plug-in into the desktop app", async () => {
  const data = tempDir("desktop-data");
  const config = tempDir("desktop-config");
  const { page } = await launch({ args: ["--data-dir", data, "--config-dir", config] });
  await skipFirstRun(page);
  const endpointFile = path.join(data, "ui-endpoint.json");
  expect(existsSync(endpointFile)).toBe(false);

  // Off by default; switching it on starts the loopback endpoint.
  const settings = await openSettingsTab(page, "Tray and startup");
  await settings
    .getByRole("checkbox", { name: "Allow “nail plugin dev” to load plug-ins into this app" })
    .click();
  await expect(settings.locator('[data-part="desktop-settings.endpoint"]')).toContainText(
    /Listening on http:\/\/127\.0\.0\.1:\d+/,
  );
  await expect.poll(() => existsSync(endpointFile)).toBe(true);
  const endpoint = JSON.parse(readFileSync(endpointFile, "utf8")) as {
    url: string;
    token?: string;
  };
  expect(endpoint.url).toMatch(/^http:\/\/127\.0\.0\.1:\d+/);
  expect(endpoint.token).toMatch(/^[A-Za-z0-9_-]{20,}$/);
  // Without the token the endpoint refuses (other programs, browsers).
  const refused = await fetch(endpoint.url).catch(() => null);
  expect(refused === null || refused.status >= 400).toBe(true);

  const folder = writePluginFolder({
    manifest: commandManifest(
      "desktop-hello",
      "Desktop hello",
      {
        id: "hello",
        title: "Say hello",
      },
      [],
    ),
    main: `export default {
      activate(ctx) {
        nail.log.info("activated on the desktop");
        ctx.commands.register("hello", () => {});
      },
    };`,
  });
  mkdirSync(path.join(folder, "locales"), { recursive: true });
  const dev = pluginDevConsole(folder, ["--data-dir", data]);
  try {
    await dev.waitFor("Loaded Desktop hello 1.0.0");
    await dev.waitFor("activated on the desktop");
    await openSettingsTab(page, "Plug-ins");
    const entry = settings.locator('[data-part="plugin-manager.plugin"][data-state="running"]');
    await expect(entry).toContainText("Desktop hello 1.0.0");
    await expect(entry).toContainText("Loaded from the development folder");
  } finally {
    dev.child.kill();
  }

  // Switched off again: the endpoint file is gone.
  await openSettingsTab(page, "Tray and startup");
  await settings
    .getByRole("checkbox", { name: "Allow “nail plugin dev” to load plug-ins into this app" })
    .click();
  await expect.poll(() => existsSync(endpointFile)).toBe(false);
});
