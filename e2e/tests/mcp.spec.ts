import { expect, test } from "@playwright/test";
import { McpSession, createMcpClient, grantMcpAccess } from "../support/mcp";
import { nailPerTest } from "../support/nail";
import { messageRows } from "../support/views";

/**
 * The read-only MCP server (ADR 0025) started by `nail serve` with the core
 * as its data provider: a client created in the settings gets nothing
 * until it is granted access, and then reads accounts and messages.
 */

const server = nailPerTest({ seed: "demo" });

test("an MCP client lists messages only after a grant", async ({ page }) => {
  await page.goto(server.url);
  await expect(messageRows(page).first()).toBeVisible();
  // A new client: its URL and token are shown.
  const { settings, url, token, access } = await createMcpClient(page, "Playwright agent");

  // Before a grant: connected, but every read is denied.
  const mcp = new McpSession(url, token);
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
  await grantMcpAccess(access, ["Read mail"]);

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
