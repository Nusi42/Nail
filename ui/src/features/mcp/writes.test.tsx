/**
 * M4a additions to the MCP client settings: writing scopes, rate limits,
 * the confirmation policy and the "waiting for you" note.
 */
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { createEnv, renderNode, widget, type Env } from "../../test/harness";
import { CENTER_DIALOG, seedAgentsDemo } from "../agents";
import { DEFAULT_RATE_LIMITS, rateLimitIssues, WRITE_SCOPES } from "./queries";
import { runtimeIds } from "./messages";

async function settle(env: Env) {
  await waitFor(() => expect(env.queryClient.isFetching()).toBe(0));
}

function demo(env: Env) {
  const client = env.transport.data.features.mcp.clients[0];
  if (!client) throw new Error("no demo client");
  return client;
}

describe("MCP client settings (M4a)", () => {
  it("grants writing scopes next to reading ones, in a stable order", async () => {
    const env = createEnv();
    const user = userEvent.setup();
    renderNode(widget("mcp-clients"), env);
    await settle(env);
    const writes = screen.getByRole("group", { name: "What it may change" });
    for (const scope of WRITE_SCOPES) {
      expect(writes.querySelector(`[data-scope="${scope}"][data-write]`)).not.toBeNull();
    }
    await user.click(within(writes).getByRole("checkbox", { name: "Send mail" }));
    await user.click(within(writes).getByRole("checkbox", { name: "Organize mail" }));
    await user.click(screen.getByRole("button", { name: "Save access" }));
    await settle(env);
    expect(demo(env).grant.scopes).toEqual(["read", "context", "organize", "send"]);
    expect(runtimeIds()).toContain("mcp-scope-permanent-delete-hint");
  });

  it("edits rate limits, with no limit and the core's checks", async () => {
    const env = createEnv();
    const user = userEvent.setup();
    renderNode(widget("mcp-clients"), env);
    await settle(env);
    const form = screen.getByRole("form", { name: "Limits for Claude Desktop" });
    const sends = within(form).getByRole("spinbutton", { name: "Messages sent per hour" });
    expect(sends).toHaveValue(10);
    await user.clear(sends);
    await user.type(sends, "0");
    expect(within(form).getByRole("alert")).toHaveTextContent(
      "A limit cannot be 0. Remove the client's permissions instead.",
    );
    expect(within(form).getByRole("button", { name: "Save limits" })).toBeDisabled();
    await user.clear(sends);
    await user.type(sends, "3");
    await user.click(within(form).getByRole("checkbox", { name: "No limit for calls" }));
    expect(within(form).queryByRole("spinbutton", { name: "Calls per minute" })).toBeNull();
    await user.click(within(form).getByRole("button", { name: "Save limits" }));
    await settle(env);
    expect(demo(env).rate_limits).toEqual({
      requests_per_minute: null,
      writes_per_minute: 20,
      sends_per_hour: 3,
    });
    expect(await within(form).findByRole("status")).toHaveTextContent("Limits saved.");
    await user.click(within(form).getByRole("button", { name: "Use the default limits" }));
    expect(within(form).getByRole("spinbutton", { name: "Calls per minute" })).toHaveValue(120);
  });

  it("checks limits like the core", () => {
    expect(rateLimitIssues(DEFAULT_RATE_LIMITS)).toEqual([]);
    expect(
      rateLimitIssues({ requests_per_minute: 0, writes_per_minute: 20_000, sends_per_hour: null }),
    ).toEqual(["zero", "too-high"]);
  });

  it("sets the client's confirmation policy and points to waiting confirmations", async () => {
    const env = createEnv();
    seedAgentsDemo(env.transport);
    const user = userEvent.setup();
    renderNode(widget("mcp-clients"), env);
    await settle(env);
    const note = screen
      .getAllByRole("note")
      .find((n) => n.getAttribute("data-part") === "mcp-clients.pending");
    if (!note) throw new Error("no pending note");
    expect(note).toHaveTextContent("Claude Desktop is waiting for 2 confirmations.");
    await user.click(within(note).getByRole("button", { name: "Review" }));
    expect(env.store.getState().windows).toContain(CENTER_DIALOG);
    const policy = screen.getByRole("region", {
      name: "What Claude Desktop may do without asking",
    });
    const draft = policy.querySelector<HTMLElement>('[data-kind="draft"]');
    if (!draft) throw new Error("no draft row");
    await user.click(within(draft).getByRole("radio", { name: /Ask first/ }));
    await waitFor(() =>
      expect(env.transport.data.features.agents.policies["mcp-client:mcp-1"]).toEqual({
        draft: "ask",
      }),
    );
    expect(policy.querySelector('[data-kind="permanent-delete"]')).toHaveAttribute(
      "data-locked",
      "",
    );
  });
});
