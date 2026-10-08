import { expect, type Locator, type Page } from "@playwright/test";
import { McpSession, type JsonRpcResponse, createMcpClient, grantMcpAccess } from "../support/mcp";
import { nailFixture, control, serverMessages, type NailServer } from "../support/nail";
import { axeViolations } from "../support/axe";
import { openSettings, rail, showView, closeWindow, openMail } from "../support/views";
import { defined } from "../support/defined";

/**
 * Wave M4a (plan WP105, ADR 0084): an AI agent (MCP client) writes through
 * the core on a private `nail serve --seed demo-multi`. A draft needs no
 * confirmation; sending waits for the user's approval in Nail's
 * confirmation center ("always allow" then lets the next send run without
 * asking); a denied delete changes nothing; a permanent delete always asks
 * and can never be always-allowed; the `summarize_thread` prompt returns
 * the thread; the client's rate limit, saved in the settings, trips.
 */
const test = nailFixture({ seed: "demo-multi" });

/** The rail's confirmations badge (M5b; absent while nothing waits). */
const confirmationsBadge = (page: Page) =>
  page.locator('[data-part="nav-rail.badge"][data-badge="confirmations"]');

/**
 * Opens Nail and creates the MCP client "Claude" in Settings → MCP clients with the given
 * permissions for all accounts and opens an MCP session for it. Leaves the
 * settings window open.
 */
async function agent(
  page: Page,
  nail: NailServer,
  scopes: string[],
): Promise<{ mcp: McpSession; settings: Locator }> {
  await openMail(page, nail.url);
  const client = await createMcpClient(page, "Claude");
  await grantMcpAccess(client.access, scopes);
  const mcp = new McpSession(client.url, client.token);
  await mcp.open();
  return { mcp, settings: client.settings };
}

/** A tool call's structured result. */
function result(response: JsonRpcResponse): Record<string, unknown> {
  expect(response.error, JSON.stringify(response.error)).toBeUndefined();
  return (response.result?.structuredContent ?? {}) as Record<string, unknown>;
}

function call(mcp: McpSession, name: string, args: Record<string, unknown>) {
  return mcp.request("tools/call", { name, arguments: args });
}

/** Opens the waiting confirmation from its notice ("Review"). */
async function review(page: Page): Promise<Locator> {
  const notices = page.getByRole("region", { name: "Waiting for your confirmation" });
  await notices.getByRole("button", { name: "Review" }).first().click();
  const center = page.getByRole("dialog", { name: "Confirmations" });
  await expect(center).toBeVisible();
  return center;
}

async function draft(mcp: McpSession, subject: string): Promise<string> {
  const created = result(
    await call(mcp, "create_draft", {
      identity_id: "ada@work.example",
      to: ["Bob <bob@example.org>"],
      subject,
      body: "Hello **Bob**, here is the plan.",
    }),
  );
  expect(created.status).toBe("done");
  expect(created.confirmation_id).toBeUndefined();
  return defined((created.created as string[])[0], "the created message");
}

async function smtpSubjects(server: NailServer): Promise<string> {
  const sent = await control<{ data: string }[]>(server, "smtp-messages");
  return sent.map((m) => m.data).join("\n");
}

test("an agent's draft needs no confirmation, sending waits for approval and always-allow relaxes it", async ({
  page,
  nail,
}) => {
  const { mcp } = await agent(page, nail, ["Read mail", "Write drafts", "Send mail"]);
  await showView(page, "Mail");

  // A draft runs without asking.
  const first = await draft(mcp, "Agent plan");
  await expect(confirmationsBadge(page)).toHaveCount(0);

  // Sending asks: the call waits while Nail shows the request.
  const sending = call(mcp, "send_message", { draft_id: first });
  await expect(confirmationsBadge(page)).toHaveText("1");
  await expect(
    rail(page).getByRole("button", { name: /one change waiting for your approval/ }),
  ).toBeVisible();
  const center = await review(page);
  const card = center.locator('[data-part="confirmation"]').first();
  await expect(card).toHaveAttribute("data-risk", "outgoing");
  await expect(card).toContainText("AI agent “Claude” (MCP client)");
  await expect(card).toContainText("Agent plan");
  expect(await axeViolations(page), "confirmation center with a pending send").toEqual([]);
  await card.getByRole("checkbox", { name: /without asking: Send mail/ }).check();
  await card.getByRole("button", { name: "Approve", exact: true }).click();
  const sent = result(await sending);
  expect(sent.status).toBe("done");
  expect(sent.confirmation_id).toBeDefined();
  await expect.poll(() => smtpSubjects(nail), { timeout: 30_000 }).toContain("Subject: Agent plan");
  await closeWindow(page.getByRole("dialog", { name: "Confirmations" }));

  // "Always allow" stored the client's policy: the next send runs at once.
  const second = await draft(mcp, "Agent follow-up");
  const again = result(await call(mcp, "send_message", { draft_id: second }));
  expect(again.status).toBe("done");
  expect(again.confirmation_id).toBeUndefined();
  const settings = await openSettings(page, "Agents (MCP)");
  const policy = settings.getByRole("region", { name: "What Claude may do without asking" });
  await expect(
    policy.getByRole("group", { name: "Send mail" }).getByRole("radio", { name: "Without asking" }),
  ).toBeChecked();
});

test("a denied delete changes nothing and a permanent delete always asks", async ({
  page,
  nail,
}) => {
  const { mcp } = await agent(page, nail, [
    "Read mail",
    "Organize mail",
    "Delete mail permanently",
  ]);
  await showView(page, "Mail");
  const found = await call(mcp, "search_messages", { query: "subject:Welcome" });
  const id = /"id":"([^"]+)"/.exec(JSON.stringify(found.result))?.[1];
  expect(id).toBeDefined();
  const before = (await serverMessages(nail, "INBOX")).length;

  // Trash asks; the user denies; nothing moves.
  const trashing = call(mcp, "delete_messages", { message_ids: [id] });
  let center = await review(page);
  let card = center.locator('[data-part="confirmation"]').first();
  await expect(card).toHaveAttribute("data-risk", "destructive");
  await expect(card).toContainText("Deleting always asks first");
  await expect(card.getByRole("checkbox")).toHaveCount(0);
  await card.getByRole("button", { name: "Deny" }).click();
  expect(result(await trashing).status).toBe("denied");
  expect((await serverMessages(nail, "INBOX")).length).toBe(before);
  await closeWindow(page.getByRole("dialog", { name: "Confirmations" }));

  // Permanent delete asks too, with the danger button and no always-allow.
  const deleting = call(mcp, "delete_messages", { message_ids: [id], permanent: true });
  center = await review(page);
  card = center.locator('[data-part="confirmation"][data-state="pending"]').first();
  await expect(card.getByRole("button", { name: "Approve and delete" })).toBeVisible();
  await expect(card.getByRole("checkbox")).toHaveCount(0);
  await card.getByRole("button", { name: "Deny" }).click();
  expect(result(await deleting).status).toBe("denied");
  expect((await serverMessages(nail, "INBOX")).length).toBe(before);
  // Both decisions are in the history.
  await center.getByRole("button", { name: "History" }).click();
  await expect(center.locator('[data-part="confirmation"][data-state="denied"]')).toHaveCount(2);
});

test("the summarize_thread prompt returns the thread and the rate limit trips", async ({
  page,
  nail,
}) => {
  const { mcp, settings } = await agent(page, nail, ["Read mail", "Write drafts"]);

  // The prompt carries the thread's messages, marked as untrusted data.
  const found = await call(mcp, "search_messages", { query: "subject:Welcome" });
  const text = JSON.stringify(found.result);
  const thread = /"thread_id":"([^"]+)"/.exec(text)?.[1];
  const subject = /"subject":"([^"]+)"/.exec(text)?.[1];
  expect(thread).toBeDefined();
  const prompt = await mcp.request("prompts/get", {
    name: "summarize_thread",
    arguments: { thread_id: thread },
  });
  expect(prompt.error).toBeUndefined();
  const messages = JSON.stringify(prompt.result?.messages);
  expect(messages).toContain(defined(subject, "the subject"));
  expect(messages).toContain("untrusted");

  // At most 2 changes per minute, saved in the client's settings.
  const limits = settings.getByRole("form", { name: "Limits for Claude" });
  await limits.getByRole("spinbutton", { name: /Changes per minute/ }).fill("2");
  await limits.getByRole("button", { name: "Save limits" }).click();
  await expect(limits.getByRole("status")).toHaveText("Limits saved.");
  await draft(mcp, "One");
  await draft(mcp, "Two");
  const third = await call(mcp, "create_draft", {
    identity_id: "ada@work.example",
    to: ["bob@example.org"],
    subject: "Three",
    body: "Too many",
  });
  const refused = (third.result?.structuredContent ?? {}) as Record<string, unknown>;
  expect(refused.error).toBe("rate_limited");
  expect(refused.limit).toBe("writes_per_minute");
  // The activity shows the refusal as denied.
  await settings.getByRole("button", { name: "Refresh" }).first().click();
  await expect(
    settings
      .locator('[data-part="mcp-audit.row"][data-outcome="denied"]')
      .filter({ hasText: "create_draft" })
      .first(),
  ).toBeVisible();
});
