import { spawn, type ChildProcess } from "node:child_process";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import path from "node:path";
import { createInterface } from "node:readline";
import AxeBuilder from "@axe-core/playwright";
import { expect, test as base, type Locator, type Page } from "@playwright/test";
import { nailBinary, startNail, type NailServer } from "../support/nail";
import { commandManifest, followUpPackage, packFiles, writePluginFolder } from "../support/plugins";
import {
  openSettingsTab,
  readerAction,
  readerToolbar,
  showView,
  viewRegion,
} from "../support/views";

/**
 * Plug-ins (plan WP64 / WP105, ADRs 0055, 0084) on a private `nail serve
 * --seed demo`: installing the full "Follow up" example through the install
 * dialog with its permissions, its reader toolbar button and dialog, the
 * task it creates (a plug-in write through the core) and the notification,
 * the placement written to layout.kdl; a plug-in without `mail.read.body`
 * refused a body; a runaway plug-in stopped and flagged while the UI keeps
 * working; `nail plugin dev` loading a folder and reloading it on changes.
 */
const test = base.extend<{ nail: NailServer }>({
  // eslint-disable-next-line no-empty-pattern -- Playwright requires a destructuring pattern for fixtures without dependencies
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

/** The settings view at the section of a former settings tab (M5b). */
function openSettings(page: Page, tab: string): Promise<Locator> {
  return openSettingsTab(page, tab);
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

test("the full Follow-up example installs with its permissions, creates a task and is placed in layout.kdl", async ({
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
  await expect(dialog.getByRole("heading", { name: "Follow up 2.0.0" })).toBeVisible();
  await expect(dialog.getByText("Command “Follow up…”")).toBeVisible();
  const permissions = dialog.getByRole("list", { name: "Permissions" });
  await expect(permissions.getByRole("listitem")).toHaveCount(6);
  // Writing tasks is marked as a change to your data.
  await expect(permissions.locator("[data-write]")).toHaveCount(1);
  for (const name of [
    /^Read your mail/,
    /tasks/i,
    "Show dialogs",
    "Store its own data on this device",
    "Show notifications",
    /background/i,
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
  // M5b: the ranked reader bar (`item "mail.forward" rank=3`).
  expect(layout).toMatch(/item "mail\.forward"[^\n]*\s+item "plugin\.follow-up\.follow-up"/);
  expect(layout).toContain('bind "shift+f" "plugin.follow-up.follow-up"');
  await dialog.getByRole("button", { name: "Close" }).first().click();
  await expect(
    settings.locator('[data-part="plugin-manager.plugin"][data-state="running"]'),
  ).toBeVisible();
  await showView(page, "Mail");

  // Its button sits in the reader toolbar; it opens the plug-in's dialog.
  const row = rows(page).filter({ hasText: "Welcome to Nail" });
  await row.click();
  // Unranked, it may sit in "More" when the bar is narrow (M5b).
  await readerAction(page, "Follow up…");
  const plugin = page.getByRole("dialog", { name: "Follow up" });
  await expect(plugin.getByText("from Follow up")).toBeVisible();
  await expect(plugin.getByText(/Follow up on “Welcome to Nail/)).toBeVisible();
  expect(await violations(page), "plug-in dialog").toEqual([]);
  await plugin.getByLabel("Due", { exact: true }).fill("2031-03-05");
  await plugin.getByLabel("Note").fill("Answer Ada");
  await plugin.getByRole("button", { name: "Create task" }).click();
  await expect(plugin).toBeHidden();

  // The notification confirms the task (created without asking: task
  // writes run without a confirmation by default)…
  const toast = page.locator('[data-part="plugin.notification"]');
  await expect(toast.filter({ hasText: "Follow-up task created, due 2031-03-05" })).toBeVisible();
  expect(await violations(page), "plug-in notification").toEqual([]);
  // …the dialog now knows about it…
  await readerAction(page, "Follow up…");
  await expect(
    plugin.getByText("A follow-up task for this message is due 2031-03-05."),
  ).toBeVisible();
  await plugin.getByRole("button", { name: "Cancel" }).click();
  await expect(plugin).toBeHidden();
  // …and the task is in Nail's task list, linked to the message.
  await page.keyboard.press("g");
  await page.keyboard.press("t");
  const tasks = viewRegion(page, "Tasks");
  await expect(tasks.getByText(/Follow up: Welcome to Nail/)).toBeVisible();
  // The plug-in's log records the write.
  await showView(page, "Mail");
  const log = await openSettings(page, "Plug-ins");
  await log.getByRole("button", { name: "Log" }).click();
  await expect(log.getByText("Changed your data: nail.tasks.create")).toBeVisible();
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
  await showView(page, "Mail");

  await rows(page).filter({ hasText: "Welcome to Nail" }).click();
  await readerAction(page, "Peek at the body");
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
  await showView(page, "Mail");

  await rows(page).filter({ hasText: "Welcome to Nail" }).click();
  await readerAction(page, "Spin forever");
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
  // Its action cannot run until it is turned on again: the reader bar
  // hides it (M5b `hide-disabled`), in "More" too.
  await showView(page, "Mail");
  await expect(page.getByRole("button", { name: "Spin forever" })).toHaveCount(0);
  await readerToolbar(page).getByRole("button", { name: "More", exact: true }).click();
  const spin = page.getByRole("menuitem", { name: /^Spin forever/ });
  if ((await spin.count()) > 0) await expect(spin).toBeDisabled();
  await page.keyboard.press("Escape");
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
  createInterface({ input: child.stdout }).on("line", (line) => lines.push(line));
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
  // Its name and command title come from its own locale file.
  const manifest = (version: string) => {
    const base = commandManifest(
      "dev-hello",
      "%plugin-name%",
      { id: "hello", title: "%hello-title%" },
      ["notifications"],
      version,
    );
    const contributes = base.contributes as Record<string, unknown>;
    return { ...base, contributes: { ...contributes, locales: { en: "locales/en.ftl" } } };
  };
  const locale = (name: string, title: string) =>
    [`plugin-name = ${name}`, `hello-title = ${title}`, ""].join("\n");
  const main = (greeting: string) => `export default {
    activate(ctx) {
      nail.log.info("activated: ${greeting}");
      ctx.commands.register("hello", () => nail.ui.notify({ title: "${greeting}" }));
    },
  };`;
  const folder = writePluginFolder({ manifest: manifest("1.0.0"), main: main("Hello one") });
  mkdirSync(path.join(folder, "locales"));
  const ftl = path.join(folder, "locales", "en.ftl");
  writeFileSync(ftl, locale("Dev hello", "Say hello"));
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
    await showView(page, "Mail");
    await rows(page).filter({ hasText: "Welcome to Nail" }).click();
    await readerAction(page, "Say hello");
    const toasts = page.locator('[data-part="plugin.notification"]');
    await expect(toasts.filter({ hasText: "Hello one" })).toBeVisible();

    // Hot reload: new code (and a new version) without reinstalling.
    writeFileSync(path.join(folder, "manifest.json"), JSON.stringify(manifest("1.0.1"), null, 2));
    writeFileSync(path.join(folder, "index.js"), main("Hello two"));
    await dev.waitFor("activated: Hello two");
    await dev.waitFor("changed; reloading");
    // The host's lines name the plug-in by its resolved name.
    await dev.waitFor("Dev hello is being reloaded.");
    expect(dev.lines.filter((l) => l.includes("%plugin-name%"))).toEqual([]);
    await readerAction(page, "Say hello");
    await expect(toasts.filter({ hasText: "Hello two" })).toBeVisible();
    const again = await openSettings(page, "Plug-ins");
    await expect(again.locator('[data-part="plugin-manager.plugin"]')).toContainText(
      "Dev hello 1.0.1",
    );
    await showView(page, "Mail");

    // A change of only its locale file (same manifest, same version)
    // reloads its texts too.
    const reloads = dev.lines.filter((l) => l.includes("changed; reloading")).length;
    writeFileSync(ftl, locale("Dev hello edited", "Say hello again"));
    await expect
      .poll(() => dev.lines.filter((l) => l.includes("changed; reloading")).length, {
        timeout: 20_000,
      })
      .toBeGreaterThan(reloads);
    // The action's new label (in the bar or, unranked, in its More menu).
    await expect(async () => {
      const button = readerToolbar(page).getByRole("button", { name: "Say hello again" });
      if ((await button.count()) > 0) return;
      await readerToolbar(page).getByRole("button", { name: "More", exact: true }).click();
      await expect(page.getByRole("menuitem", { name: /^Say hello again/ })).toBeVisible({
        timeout: 1_000,
      });
      await page.keyboard.press("Escape");
    }).toPass({ timeout: 15_000 });
    const edited = await openSettings(page, "Plug-ins");
    await expect(edited.locator('[data-part="plugin-manager.plugin"]')).toContainText(
      "Dev hello edited 1.0.1",
    );
    expect(dev.lines.filter((l) => l.includes("%plugin-name%"))).toEqual([]);
  } finally {
    dev.child.kill();
  }
});
