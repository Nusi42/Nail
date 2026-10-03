import { expect, test } from "@playwright/test";
import { McpSession } from "../support/mcp";
import { startNail, type NailServer } from "../support/nail";
import { openSettingsTab, showView, viewRegion } from "../support/views";

/**
 * The read-only MCP server (ADR 0025) started by `nail serve` with the core
 * as its data provider: a client created in the settings gets nothing
 * until it is granted access, and then reads accounts and messages.
 */

let server: NailServer;

test.beforeEach(async () => {
  server = await startNail({ seed: "demo" });
});

test.afterEach(async () => {
  await server.stop();
});

test("an MCP client lists messages only after a grant", async ({ page }) => {
  await page.goto(server.url);
  await expect(page.locator('[data-part="message-list.row"]').first()).toBeVisible();
  await showView(page, "Settings");
  const settings = viewRegion(page, "Settings");
  await openSettingsTab(page, "MCP clients");

  // The server runs; its URL is shown.
  const serverLine = settings.locator('[data-part="mcp-clients.server"]');
  await expect(serverLine).toHaveAttribute("data-state", "running");
  const url = /(http:\/\/127\.0\.0\.1:\d+\/mcp)/.exec((await serverLine.textContent()) ?? "")?.[1];
  expect(url).toBeDefined();

  // Create a client; its token is shown once.
  await settings.getByRole("textbox", { name: "Client name" }).fill("Playwright agent");
  await settings.getByRole("button", { name: "Create client" }).click();
  const token = (await settings.locator('[data-part="mcp-token.value"]').textContent())?.trim();
  expect(token).toMatch(/^nail_mcp_/);

  // Before a grant: connected, but every read is denied.
  const mcp = new McpSession(url!, token!);
  const init = await mcp.open();
  expect(init.result?.serverInfo).toMatchObject({ name: "nail" });
  const denied = await mcp.request("tools/call", {
    name: "search_messages",
    arguments: { query: "welcome" },
  });
  const deniedText = JSON.stringify(denied);
  expect(denied.error?.data?.nail ?? deniedText).toMatch(/denied/);
  const deniedRead = await mcp.request("resources/read", { uri: "nail://accounts" });
  expect(deniedRead.error?.data?.nail).toBe("denied");

  // Grant "Read mail" for all accounts in the settings.
  const form = settings.getByRole("form", { name: "Access of Playwright agent" });
  await form.getByRole("checkbox", { name: "Read mail" }).check();
  await form.getByRole("checkbox", { name: /All accounts/ }).check();
  await form.getByRole("button", { name: "Save access" }).click();
  await expect(form.getByRole("status")).toHaveText("Access saved.");

  // Now the same session reads accounts and finds messages.
  const accounts = await mcp.request("resources/read", { uri: "nail://accounts" });
  expect(accounts.error).toBeUndefined();
  expect(JSON.stringify(accounts.result)).toContain("demo");
  const found = await mcp.request("tools/call", {
    name: "search_messages",
    arguments: { query: "welcome" },
  });
  expect(found.error).toBeUndefined();
  expect(JSON.stringify(found.result)).toContain("Welcome to Nail");
  // Mailbox content is marked as untrusted data.
  expect(JSON.stringify(found.result)).toContain("untrusted_content");

  // The activity log shows the denied and the allowed calls.
  const audit = settings.locator('[data-part="mcp-audit.row"]');
  await settings.getByRole("button", { name: "Refresh" }).first().click();
  await expect(audit.filter({ hasText: "search_messages" }).first()).toBeVisible();
  await expect(
    settings.locator('[data-part="mcp-audit.row"][data-outcome="denied"]').first(),
  ).toBeVisible();
});
