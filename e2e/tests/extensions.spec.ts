import { expect, type Page } from "@playwright/test";
import { nailFixture, control, serverMessages } from "../support/nail";
import { axeViolations } from "../support/axe";
import { examplePackage, packFiles, installPlugin } from "../support/plugins";
import { openSettingsTab, showView, messageRows } from "../support/views";
import { newMessage } from "../support/compose";

/**
 * Wave M4a plug-in extension points wired into the core (plan WP105, ADR
 * 0084) on a private `nail serve --seed demo-multi`: the before-send
 * example vetoes a mail without its attachment and "Send anyway" sends it;
 * an arrival hook tags new mail (through the plug-in write path and the
 * operation queue, so the tag reaches the IMAP server) and its open hook's
 * banner button reaches the plug-in; the attachment preview example
 * renders a CSV under its localized renderer title; a background job (with
 * a localized `%id%` title) runs on schedule on the controlled clock and
 * "Run now" runs it again; a runaway arrival hook is stopped while the
 * mail still arrives.
 */
const test = nailFixture({ seed: "demo-multi" });

/** Installs a package with every permission allowed; closes the settings. */
async function install(page: Page, file: string, title: RegExp): Promise<void> {
  const settings = await installPlugin(page, file, title);
  await expect(
    settings.locator('[data-part="plugin-manager.plugin"][data-state="running"]').last(),
  ).toBeVisible();
  await showView(page, "Mail");
}

/** An RFC 5322 message for `imap-deliver`. */
function mail(subject: string, body = "Hello Ada,\r\n\r\nsee you.\r\n"): string {
  return [
    "From: Grace Hopper <grace@navy.example>",
    "To: Ada <ada@work.example>",
    `Subject: ${subject}`,
    `Message-ID: <${subject.replace(/\W+/g, "-")}@navy.example>`,
    "Date: Thu, 01 Oct 2026 09:00:00 +0000",
    "MIME-Version: 1.0",
    "Content-Type: text/plain; charset=utf-8",
    "",
    body,
  ].join("\r\n");
}

test("the before-send example vetoes a mail without its attachment and Send anyway works", async ({
  page,
  nail,
}) => {
  await page.goto(nail.url);
  await expect(messageRows(page).first()).toBeVisible();
  await install(page, examplePackage("attachment-check"), /^Attachment check 1\.0\.0$/);

  const composer = await newMessage(page, {
    subject: "Quarterly report",
    text: "Hi Grace,\n\nplease see the file from yesterday.\n\nAda",
  });
  await composer.getByRole("button", { name: "Send", exact: true }).click();

  // The veto names the plug-in and its reason; nothing was sent.
  const veto = page.getByRole("dialog", { name: "A plug-in stopped this message" });
  await expect(veto).toBeVisible();
  // M5c: a modal confirmation of the composer it came from (only that
  // composer is blocked).
  await expect(veto).toHaveAttribute("data-owner", /^composer:/);
  await expect(veto).toContainText("Plug-in “Attachment check”:");
  await expect(veto).toContainText("You mention “see the file”, but nothing is attached.");
  expect(await axeViolations(page, '[data-part="plugin-veto"]'), "veto dialog").toEqual([]);
  await veto.getByRole("button", { name: "Keep editing" }).click();
  await expect(veto).toBeHidden();
  await expect(composer.getByRole("textbox", { name: "Subject" })).toHaveValue("Quarterly report");
  expect(await control<unknown[]>(nail, "smtp-messages")).toEqual([]);

  // "Send anyway" sends it.
  await composer.getByRole("button", { name: "Send", exact: true }).click();
  await expect(veto).toBeVisible();
  await veto.getByRole("button", { name: "Send anyway" }).click();
  await expect(composer).toBeHidden();
  const status = page.getByRole("status", { name: "Status" });
  await expect(status.getByText("Message sent.")).toBeVisible({ timeout: 20_000 });
  const sent = await control<{ data: string }[]>(nail, "smtp-messages");
  expect(sent.map((m) => m.data).join()).toContain("Subject: Quarterly report");
});

test("an arrival hook tags new mail and an open hook's banner button reaches the plug-in", async ({
  page,
  nail,
}) => {
  const file = packFiles({
    manifest: {
      id: "triage",
      name: "Triage helper",
      version: "1.0.0",
      apiVersion: "1",
      main: "index.js",
      permissions: ["mail.read", "mail.organize", "hooks.arrival", "hooks.open", "notifications"],
      contributes: { hooks: [{ hook: "arrival" }, { hook: "open" }] },
    },
    main: `export default {
      activate(ctx) {
        ctx.hooks.register("arrival", async ({ message }) =>
          message.subject.includes("Invoice") ? { actions: [{ type: "addTag", tag: "invoices" }] } : null);
        ctx.hooks.register("open", async ({ message }) =>
          message.subject.includes("Invoice")
            ? { banners: [{ id: "pay", level: "warning", text: "This looks like an invoice.",
                buttons: [{ id: "ok", label: "Got it" }] }], badges: [{ text: "Invoice" }] }
            : null);
        ctx.hooks.onBannerAction(async ({ buttonId }) => {
          nail.ui.notify({ title: "Banner button " + buttonId });
        });
      },
    };`,
  });
  await page.goto(nail.url);
  await expect(messageRows(page).first()).toBeVisible();
  await install(page, file, /^Triage helper 1\.0\.0$/);

  await control(nail, "imap-deliver", { raw: mail("Invoice 2026-10") });
  const row = messageRows(page).filter({ hasText: "Invoice 2026-10" });
  await expect(row).toBeVisible({ timeout: 20_000 });
  // The tag went through the operation queue to the server.
  await expect
    .poll(
      async () =>
        (await serverMessages(nail, "INBOX")).find((m) => m.subject === "Invoice 2026-10")?.flags ??
        [],
      { timeout: 20_000 },
    )
    .toContain("invoices");

  // Opening it shows the plug-in's banner and badge, each naming it.
  await row.click();
  const banners = page.getByRole("list", { name: "Notes from plug-ins" });
  await expect(banners).toContainText("Triage helper");
  await expect(banners).toContainText("This looks like an invoice.");
  await expect(page.locator('[data-part="plugin-badge"]')).toContainText("Invoice");
  await banners.getByRole("button", { name: "Got it" }).click();
  await expect(
    page.locator('[data-part="plugin.notification"]').filter({ hasText: "Banner button ok" }),
  ).toBeVisible();
});

test("the attachment preview example renders a CSV", async ({ page, nail }) => {
  await page.goto(nail.url);
  await expect(messageRows(page).first()).toBeVisible();
  await install(page, examplePackage("csv-preview"), /^CSV preview 1\.0\.0$|^CSV/);
  const raw = [
    "From: Grace Hopper <grace@navy.example>",
    "To: Ada <ada@work.example>",
    "Subject: Team list",
    "Message-ID: <team-list@navy.example>",
    "Date: Thu, 01 Oct 2026 09:00:00 +0000",
    "MIME-Version: 1.0",
    'Content-Type: multipart/mixed; boundary="b1"',
    "",
    "--b1",
    "Content-Type: text/plain; charset=utf-8",
    "",
    "The list is attached.",
    "--b1",
    'Content-Type: text/csv; name="team.csv"',
    'Content-Disposition: attachment; filename="team.csv"',
    "",
    "Name,Role",
    "Ada,Engineer",
    "Grace,Admiral",
    "--b1--",
    "",
  ].join("\r\n");
  await control(nail, "imap-deliver", { raw });
  const row = messageRows(page).filter({ hasText: "Team list" });
  await expect(row).toBeVisible({ timeout: 20_000 });
  await row.click();
  const preview = page.getByRole("region", { name: "Preview of team.csv" });
  await expect(preview).toBeVisible({ timeout: 20_000 });
  await expect(preview.getByRole("table")).toBeVisible();
  await expect(preview.getByRole("cell", { name: "Grace" })).toBeVisible();
  // The renderer's `%preview-title%` and the plug-in's `%plugin-name%`
  // come from the example's catalog.
  await expect(preview).toContainText("Table, shown by the plug-in CSV preview");
  await expect(preview).not.toContainText("%");
});

test("a background job runs on schedule on the controlled clock and Run now runs it again", async ({
  page,
  nail,
}) => {
  const file = packFiles({
    manifest: {
      id: "digest",
      name: "Hourly digest",
      version: "1.0.0",
      apiVersion: "1",
      main: "index.js",
      permissions: ["background", "storage"],
      contributes: {
        jobs: [{ id: "tick", title: "%tick-title%", schedule: "@hourly" }],
        locales: { en: "locales/en.ftl" },
      },
    },
    files: { "locales/en.ftl": "tick-title = Count the hours\n" },
    main: `export default {
      activate(ctx) {
        ctx.jobs.register("tick", async (trigger) => {
          const n = (await nail.storage.get("ticks")) || 0;
          await nail.storage.set("ticks", n + 1);
          nail.log.info("tick " + trigger.kind + " " + (n + 1));
        });
      },
    };`,
  });
  await page.goto(nail.url);
  await expect(messageRows(page).first()).toBeVisible();
  await install(page, file, /^Hourly digest 1\.0\.0$/);

  // Two hours later on the core's clock the hourly job has run.
  await control(nail, "clock-advance", { seconds: 7200 });
  const settings = await openSettingsTab(page, "Plug-ins");
  const entry = settings.locator('[data-part="plugin-manager.plugin"]').filter({
    hasText: "Hourly digest",
  });
  await entry.getByRole("button", { name: "Log" }).click();
  await expect(settings.getByText(/tick schedule 1/)).toBeVisible({ timeout: 20_000 });
  await entry.getByRole("button", { name: "Activity" }).click();
  const jobs = settings.getByRole("region", { name: "Background jobs" });
  // The job's `%tick-title%` title comes from the plug-in's catalog.
  await expect(jobs).toContainText("Count the hours");
  await expect(jobs).not.toContainText("%tick-title%");
  await expect(jobs).toContainText("@hourly");
  await expect(jobs).toContainText("1, none failed");

  // "Run now" runs it once more.
  await jobs.getByRole("button", { name: /Run “Count the hours” now/ }).click();
  await expect(jobs).toContainText("2, none failed", { timeout: 20_000 });
});

test("a runaway arrival hook is stopped and the mail still arrives", async ({ page, nail }) => {
  const file = packFiles({
    manifest: {
      id: "looper",
      name: "Looper",
      version: "1.0.0",
      apiVersion: "1",
      main: "index.js",
      permissions: ["mail.read", "hooks.arrival"],
      contributes: { hooks: [{ hook: "arrival", timeoutMs: 500 }] },
    },
    main: `export default {
      activate(ctx) {
        ctx.hooks.register("arrival", () => { for (;;) {} });
      },
    };`,
  });
  await page.goto(nail.url);
  await expect(messageRows(page).first()).toBeVisible();
  await install(page, file, /^Looper 1\.0\.0$/);

  await control(nail, "imap-deliver", { raw: mail("Still arrives") });
  await expect(messageRows(page).filter({ hasText: "Still arrives" })).toBeVisible({
    timeout: 20_000,
  });
  // The core stops the plug-in after its CPU budget and flags it.
  const settings = await openSettingsTab(page, "Plug-ins");
  await expect(
    settings.locator('[data-part="plugin-manager.plugin"][data-state="crashed"]'),
  ).toContainText("Stopped after a problem", { timeout: 20_000 });
  await showView(page, "Mail");
  // Mail keeps arriving.
  await control(nail, "imap-deliver", { raw: mail("Arrives after the stop") });
  await expect(messageRows(page).filter({ hasText: "Arrives after the stop" })).toBeVisible({
    timeout: 20_000,
  });
});

test("a composer action inserts text into the open draft, attributed to its plug-in", async ({
  page,
  nail,
}) => {
  const file = packFiles({
    manifest: {
      id: "greeter",
      name: "Greeter",
      version: "1.0.0",
      apiVersion: "1",
      main: "index.js",
      permissions: ["composer"],
      contributes: { composerActions: [{ id: "greet", title: "Insert a greeting" }] },
    },
    main: `export default {
      activate(ctx) {
        ctx.composer.registerAction("greet", async () => {
          await nail.composer.insert("Kind regards from the greeter");
        });
      },
    };`,
  });
  await page.goto(nail.url);
  await expect(messageRows(page).first()).toBeVisible();
  await install(page, file, /^Greeter 1\.0\.0$/);

  const composer = await newMessage(page, { subject: "Greetings" });
  await composer.getByRole("button", { name: "Plug-in actions" }).click();
  await page.getByRole("menuitem", { name: /Insert a greeting/ }).click();
  await expect(composer.getByRole("textbox", { name: "Message" })).toHaveValue(
    /Kind regards from the greeter/,
  );
  await expect(composer).toContainText("Plug-in “Greeter” inserted text.");
});
