import { spawn, type ChildProcess } from "node:child_process";
import { readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { createInterface } from "node:readline";
import AxeBuilder from "@axe-core/playwright";
import { expect, test as base, type Locator, type Page } from "@playwright/test";
import { nailBinary, startNail, type NailServer } from "../support/nail";
import { commandManifest, followUpPackage, packFiles, writePluginFolder } from "../support/plugins";

/**
 * Wave M2d plug-in preview (plan WP64, ADR 0055) on a private `nail serve
 * --seed demo`: installing the "Follow up" example through the install
 * dialog with its permissions, its reader toolbar button and dialog, the
 * stored reminder and the notification, the placement written to
 * layout.kdl; a plug-in without `mail.read.body` refused a body; a runaway
 * plug-in stopped and flagged while the UI keeps working; `nail plugin
 * dev` loading a folder and reloading it on changes.
 */
const test = base.extend<{ nail: NailServer }>({
  nail: async ({}, use) => {
    const server = await startNail();
    await use(server);
    await server.stop();
  },
});

const rows = (page: Page) => page.locator('[data-part="message-list.row"]');

/** Every axe violation (plug-in dialogs and toasts are drawn by Nail). */
async function violations(page: Page) {
  const results = await new AxeBuilder({ page }).analyze();
  return results.violations.map((v) => ({
    id: v.id,
    impact: v.impact,
    targets: v.nodes.map((n) => n.target.join(" ")),
  }));
}

async function openSettings(page: Page, tab: string): Promise<Locator> {
  await page
    .getByRole("toolbar", { name: "Main toolbar" })
    .getByRole("button", { name: "Settings" })
    .click();
  const settings = page.getByRole("dialog", { name: "Settings" });
  await settings.getByRole("tab", { name: tab, exact: true }).click();
  return settings;
}

async function closeDialog(page: Page, name: string) {
  const dialog = page.getByRole("dialog", { name });
  await dialog.getByRole("button", { name: "Close" }).first().click();
  await expect(dialog).toBeHidden();
}

/**
 * Installs a package through Settings → Plug-ins → "Install a plug-in…",
 * allowing every permission, and adds its suggested places to the layout.
 * Leaves the settings window open.
 */
async function install(page: Page, file: string, title: RegExp): Promise<Locator> {
  const settings = await openSettings(page, "Plug-ins");
  await settings.getByRole("button", { name: "Install a plug-in…" }).click();
  const dialog = page.getByRole("dialog", { name: "Install a plug-in" });
  await dialog.getByRole("textbox", { name: "Path of the .nailplugin file" }).fill(file);
  await dialog.getByRole("button", { name: "Check the plug-in" }).click();
  await expect(dialog.getByRole("heading", { name: title })).toBeVisible();
  await dialog.getByRole("button", { name: "Install", exact: true }).click();
  await expect(dialog.getByText(/is installed\./)).toBeVisible();
  await dialog.getByRole("button", { name: /^Add \d+ items? to the layout$/ }).click();
  await expect(dialog.getByText(/Added to your layout/)).toBeVisible();
  await dialog.getByRole("button", { name: "Close" }).first().click();
  await expect(dialog).toBeHidden();
  return settings;
}

test("the example installs with its permissions, runs from the reader toolbar and is placed in layout.kdl", async ({
  page,
  nail,
}) => {
  await page.goto(nail.url);
  await expect(rows(page).first()).toBeVisible();

  const settings = await openSettings(page, "Plug-ins");
  await expect(settings.getByText("No plug-ins are installed.")).toBeVisible();
  await settings.getByRole("button", { name: "Install a plug-in…" }).click();
  const dialog = page.getByRole("dialog", { name: "Install a plug-in" });
  await dialog
    .getByRole("textbox", { name: "Path of the .nailplugin file" })
    .fill(followUpPackage());
  await dialog.getByRole("button", { name: "Check the plug-in" }).click();

  // The review names the plug-in (texts from its own catalog), what it
  // adds and every permission with Allow / Deny.
  await expect(dialog.getByRole("heading", { name: "Follow up 1.0.0" })).toBeVisible();
  await expect(dialog.getByText("Command “Follow up…”")).toBeVisible();
  const permissions = dialog.getByRole("list", { name: "Permissions" });
  await expect(permissions.getByRole("listitem")).toHaveCount(4);
  for (const name of [
    /^Read your mail/,
    "Show dialogs",
    "Store its own data on this device",
    "Show notifications",
  ]) {
    await expect(
      permissions.getByRole("group", { name }).getByRole("radio", { name: "Allow" }),
    ).toBeChecked();
  }
  await dialog.getByRole("button", { name: "Install", exact: true }).click();
  await expect(dialog.getByText("Follow up is installed.")).toBeVisible();

  // The suggested placement lands in layout.kdl through the merge flow.
  await dialog.getByRole("button", { name: /^Add \d+ items? to the layout$/ }).click();
  await expect(dialog.getByText(/Added to your layout/)).toBeVisible();
  const layout = readFileSync(path.join(nail.configDir, "layout.kdl"), "utf8");
  expect(layout).toMatch(/item "mail\.forward"\s+item "plugin\.follow-up\.follow-up"/);
  expect(layout).toContain('bind "shift+f" "plugin.follow-up.follow-up"');
  await dialog.getByRole("button", { name: "Close" }).first().click();
  await expect(
    settings.locator('[data-part="plugin-manager.plugin"][data-state="running"]'),
  ).toBeVisible();
  await closeDialog(page, "Settings");

  // Its button sits in the reader toolbar; it opens the plug-in's dialog.
  const row = rows(page).filter({ hasText: "Welcome to Nail" });
  await row.click();
  const followUp = page.getByRole("button", { name: "Follow up…" });
  await expect(followUp).toBeEnabled();
  await followUp.click();
  const plugin = page.getByRole("dialog", { name: "Follow up" });
  await expect(plugin.getByText("from Follow up")).toBeVisible();
  await expect(plugin.getByText(/Remind me about “Welcome to Nail/)).toBeVisible();
  expect(await violations(page), "plug-in dialog").toEqual([]);
  await plugin.getByLabel("Due").fill("2031-03-05");
  await plugin.getByLabel("Note").fill("Answer Ada");
  await plugin.getByRole("button", { name: "Save reminder" }).click();
  await expect(plugin).toBeHidden();

  // The notification confirms it…
  const toast = page.locator('[data-part="plugin.notification"]');
  await expect(toast.filter({ hasText: "Follow-up reminder set for 2031-03-05" })).toBeVisible();
  expect(await violations(page), "plug-in notification").toEqual([]);
  // …and the reminder is stored: the dialog now offers to remove it.
  await followUp.click();
  await expect(plugin.getByRole("button", { name: "Remove reminder" })).toBeVisible();
  await expect(plugin.getByLabel("Note")).toHaveValue("Answer Ada");
  await plugin.getByRole("button", { name: "Cancel" }).click();
  await expect(plugin).toBeHidden();
});

test("a plug-in without mail.read.body is refused the message body", async ({ page, nail }) => {
  const file = packFiles({
    manifest: commandManifest("peek", "Peek", { id: "peek", title: "Peek at the body" }, [
      "mail.read",
      "notifications",
    ]),
    main: `export default {
      activate(ctx) {
        ctx.commands.register("peek", async (c) => {
          try {
            const body = await nail.mail.getBody(c.selection.openMessageId ?? c.selection.messageIds[0]);
            nail.ui.notify({ title: "Read " + body.text.length + " characters" });
          } catch (e) {
            nail.ui.notify({ title: "Refused: " + e.code, level: "error" });
          }
        });
      },
    };`,
  });
  await page.goto(nail.url);
  await expect(rows(page).first()).toBeVisible();
  await install(page, file, /^Peek 1\.0\.0$/);
  await closeDialog(page, "Settings");

  await rows(page).filter({ hasText: "Welcome to Nail" }).click();
  await page.getByRole("button", { name: "Peek at the body" }).click();
  await expect(
    page.locator('[data-part="plugin.notification"]').filter({ hasText: "Refused: forbidden" }),
  ).toBeVisible();

  // The plug-in's log names the missing permission.
  const settings = await openSettings(page, "Plug-ins");
  await settings.getByRole("button", { name: "Log" }).click();
  await expect(
    settings.getByText(
      "The plug-in called nail.mail.getBody without the permission mail.read.body.",
    ),
  ).toBeVisible();
});

test("a runaway plug-in is stopped and flagged while the UI stays responsive", async ({
  page,
  nail,
}) => {
  const file = packFiles({
    manifest: commandManifest("spinner", "Spinner", { id: "spin", title: "Spin forever" }, []),
    main: `export default {
      activate(ctx) {
        ctx.commands.register("spin", () => { for (;;) {} });
      },
    };`,
  });
  await page.goto(nail.url);
  await expect(rows(page).first()).toBeVisible();
  await install(page, file, /^Spinner 1\.0\.0$/);
  await closeDialog(page, "Settings");

  await rows(page).filter({ hasText: "Welcome to Nail" }).click();
  await page.getByRole("button", { name: "Spin forever" }).click();
  // The UI keeps working while the plug-in loops: another message opens.
  const other = rows(page).nth(1);
  const subject = (
    await other.locator('[data-part="message-list.row.subject"]').innerText()
  ).trim();
  await other.click();
  await expect(page.locator('[data-part="message-view"]')).toContainText(subject);

  // The core stops it after its CPU budget and says so.
  await expect(
    page
      .locator('[data-part="plugin.notification"]')
      .filter({ hasText: "Spinner was stopped because it took too long." }),
  ).toBeVisible({ timeout: 15_000 });
  const settings = await openSettings(page, "Plug-ins");
  const entry = settings.locator('[data-part="plugin-manager.plugin"][data-state="crashed"]');
  await expect(entry).toBeVisible();
  await expect(entry).toContainText("Stopped after a problem");
  // Its button is disabled until it is turned on again.
  await closeDialog(page, "Settings");
  await expect(page.getByRole("button", { name: "Spin forever" })).toBeDisabled();
});

/** A running `nail plugin dev` console: its stdout lines. */
interface Console {
  child: ChildProcess;
  lines: string[];
  waitFor(text: string): Promise<void>;
}

function devConsole(folder: string, url: string): Console {
  const child = spawn(nailBinary(), ["plugin", "dev", folder, "--url", url], {
    stdio: ["ignore", "pipe", "inherit"],
    env: { ...process.env, NAIL_LOG: "error", NAIL_LOCALE: "en" },
  });
  const lines: string[] = [];
  createInterface({ input: child.stdout! }).on("line", (line) => lines.push(line));
  return {
    child,
    lines,
    async waitFor(text: string) {
      await expect
        .poll(() => lines.some((l) => l.includes(text)), {
          message: `console line with "${text}"; saw ${JSON.stringify(lines)}`,
          timeout: 20_000,
        })
        .toBe(true);
    },
  };
}

test("nail plugin dev loads a folder and reloads it on changes", async ({ page, nail }) => {
  const manifest = (version: string) =>
    commandManifest(
      "dev-hello",
      "Dev hello",
      { id: "hello", title: "Say hello" },
      ["notifications"],
      version,
    );
  const main = (greeting: string) => `export default {
    activate(ctx) {
      nail.log.info("activated: ${greeting}");
      ctx.commands.register("hello", () => nail.ui.notify({ title: "${greeting}" }));
    },
  };`;
  const folder = writePluginFolder({ manifest: manifest("1.0.0"), main: main("Hello one") });
  const dev = devConsole(folder, nail.url);
  try {
    await dev.waitFor("Loaded Dev hello 1.0.0");
    await dev.waitFor("activated: Hello one");

    await page.goto(nail.url);
    await expect(rows(page).first()).toBeVisible();
    const settings = await openSettings(page, "Plug-ins");
    const entry = settings.locator('[data-part="plugin-manager.plugin"][data-state="running"]');
    await expect(entry).toContainText("Dev hello 1.0.0");
    await expect(entry).toContainText("Loaded from the development folder");
    // Placed from the manager, then run.
    await entry.getByRole("button", { name: "Place in layout" }).click();
    await settings.getByRole("button", { name: /^Add \d+ items? to the layout$/ }).click();
    await expect(settings.getByText(/Added to your layout/)).toBeVisible();
    await closeDialog(page, "Settings");
    await rows(page).filter({ hasText: "Welcome to Nail" }).click();
    await page.getByRole("button", { name: "Say hello" }).click();
    const toasts = page.locator('[data-part="plugin.notification"]');
    await expect(toasts.filter({ hasText: "Hello one" })).toBeVisible();

    // Hot reload: new code (and a new version) without reinstalling.
    writeFileSync(path.join(folder, "manifest.json"), JSON.stringify(manifest("1.0.1"), null, 2));
    writeFileSync(path.join(folder, "index.js"), main("Hello two"));
    await dev.waitFor("activated: Hello two");
    await dev.waitFor("changed; reloading");
    await page.getByRole("button", { name: "Say hello" }).click();
    await expect(toasts.filter({ hasText: "Hello two" })).toBeVisible();
    const again = await openSettings(page, "Plug-ins");
    await expect(again.locator('[data-part="plugin-manager.plugin"]')).toContainText(
      "Dev hello 1.0.1",
    );
  } finally {
    dev.child.kill();
  }
});
