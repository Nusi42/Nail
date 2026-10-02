/**
 * The confirmation center, its indicator and notices, and the policy
 * editor against the mock core (ADR 0079 rules: destructive kinds never
 * offer "always allow").
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Confirmation } from "../../api/generated";
import { createEnv, renderWithProviders, widget, type Env } from "../../test/harness";
import { NodeView } from "../../layout/LayoutRenderer";
import {
  CENTER_DIALOG,
  ConfirmationCard,
  handleAgentsEvent,
  PolicyEditor,
  requestConfirmation,
  seedAgentsDemo,
} from ".";
import { ConfirmationNotices } from "./ConfirmationNotices";
import { agentsStoreFor } from "./store";
import { HISTORY_PAGE } from "./queries";

const CLIENT = { kind: "mcp-client", client_id: "mcp-1" } as const;

function wire(env: Env): () => void {
  return env.client.subscribe((event) =>
    handleAgentsEvent(event, {
      client: env.client,
      queryClient: env.queryClient,
      store: env.store,
    }),
  );
}

function cardOf(confirmation: Confirmation): HTMLElement {
  const el = document.querySelector<HTMLElement>(`[data-confirmation="${confirmation.id}"]`);
  if (!el) throw new Error(`no card for ${confirmation.id}`);
  return el;
}

async function settle(env: Env) {
  await waitFor(() => expect(env.queryClient.isFetching()).toBe(0));
}

describe("confirmation center", () => {
  it("lists pending writes with attribution, kind, items and before / after", async () => {
    const env = createEnv();
    const [send, trash, invite] = seedAgentsDemo(env.transport);
    renderWithProviders(<NodeView node={widget("confirmation-center")} path="t" />, env);
    await settle(env);
    const sendCard = cardOf(send as Confirmation);
    expect(sendCard).toHaveAttribute("data-risk", "outgoing");
    expect(
      within(sendCard).getByText("AI agent “Claude Desktop” (MCP client)"),
    ).toBeInTheDocument();
    expect(
      within(sendCard).getByText(
        "Claude Desktop wants to send “Re: Quarterly report” to 2 recipients.",
      ),
    ).toBeInTheDocument();
    expect(within(sendCard).getByText("Grace Hopper <grace@example.org>")).toBeInTheDocument();
    expect(within(sendCard).getByRole("table", { name: "What changes" })).toBeInTheDocument();
    expect(within(sendCard).getByText("q3-figures.csv")).toBeInTheDocument();
    // Outgoing mail may be relaxed per client.
    expect(within(sendCard).getByRole("checkbox")).toBeInTheDocument();

    const trashCard = cardOf(trash as Confirmation);
    expect(trashCard).toHaveAttribute("data-risk", "destructive");
    expect(within(trashCard).queryByRole("checkbox")).toBeNull();
    expect(
      within(trashCard).getByText(
        "Deleting always asks first. This cannot be allowed without asking.",
      ),
    ).toBeInTheDocument();
    expect(
      within(trashCard).getByRole("button", { name: "Approve and delete" }),
    ).toBeInTheDocument();

    // A plug-in acting for an agent names both.
    const inviteCard = cardOf(invite as Confirmation);
    expect(within(inviteCard).getByText("Plug-in “Follow-up”")).toBeInTheDocument();
    expect(within(inviteCard).getByText("for the AI agent “Claude Desktop”")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Waiting (3)" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );
  });

  it("approves with always allow, which sets the client's policy", async () => {
    const env = createEnv();
    const [send] = seedAgentsDemo(env.transport);
    const user = userEvent.setup();
    renderWithProviders(<NodeView node={widget("confirmation-center")} path="t" />, env);
    await settle(env);
    const card = cardOf(send as Confirmation);
    await user.click(within(card).getByRole("checkbox"));
    await user.click(within(card).getByRole("button", { name: "Approve" }));
    await settle(env);
    const stored = env.transport.data.features.agents.confirmations.find((c) => c.id === send?.id);
    expect(stored?.state).toBe("approved");
    expect(stored?.decided_via).toBe("nail");
    expect(env.transport.data.features.agents.policies["mcp-client:mcp-1"]).toEqual({
      send: "allow",
    });
    await waitFor(() =>
      expect(document.querySelector(`[data-confirmation="${send?.id}"]`)).toBeNull(),
    );
  });

  it("denies a permanent delete and shows it in the history", async () => {
    const env = createEnv();
    const c = requestConfirmation(env.transport, {
      requested_by: { actor: CLIENT, name: "Claude Desktop" },
      kind: "permanent-delete",
      operation: "delete_messages",
      summary: {
        id: "confirmation-summary-permanent-delete",
        args: { actor: "Claude Desktop", count: 2 },
      },
    });
    const user = userEvent.setup();
    renderWithProviders(<NodeView node={widget("confirmation-center")} path="t" />, env);
    await settle(env);
    await user.click(within(cardOf(c)).getByRole("button", { name: "Deny" }));
    await settle(env);
    expect(env.transport.data.features.agents.confirmations[0]?.state).toBe("denied");
    expect(
      await screen.findByText("Nothing is waiting for your confirmation."),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "History" }));
    await settle(env);
    const card = cardOf(c);
    expect(within(card).getByText("Denied")).toBeInTheDocument();
    expect(within(card).getByText("in Nail")).toBeInTheDocument();
    expect(within(card).queryByRole("button", { name: "Deny" })).toBeNull();
  });

  it("explains a confirmation that was decided elsewhere", async () => {
    const env = createEnv();
    const c = requestConfirmation(env.transport, {
      requested_by: { actor: CLIENT, name: "Claude Desktop" },
      kind: "send",
      operation: "send_message",
      summary: { id: "confirmation-summary-send", args: { actor: "x", subject: "s", count: 1 } },
    });
    const user = userEvent.setup();
    renderWithProviders(<ConfirmationCard confirmation={structuredClone(c)} />, env);
    c.state = "expired";
    await user.click(screen.getByRole("button", { name: "Approve" }));
    expect(
      await screen.findByText("This was already decided, has expired or was cancelled."),
    ).toBeInTheDocument();
  });

  it("pages the history", async () => {
    const env = createEnv();
    for (let i = 0; i < HISTORY_PAGE + 5; i++) {
      const c = requestConfirmation(env.transport, {
        requested_by: { actor: CLIENT, name: "Claude Desktop" },
        kind: "organize",
        operation: "update_messages",
        summary: { id: "confirmation-summary-organize", args: { actor: "x", count: i + 1 } },
      });
      c.state = "approved";
    }
    const user = userEvent.setup();
    renderWithProviders(<NodeView node={widget("confirmation-center")} path="t" />, env);
    await user.click(screen.getByRole("button", { name: "History" }));
    await settle(env);
    expect(document.querySelectorAll('[data-part="confirmation"]')).toHaveLength(HISTORY_PAGE);
    await user.click(screen.getByRole("button", { name: "Show older" }));
    await settle(env);
    expect(document.querySelectorAll('[data-part="confirmation"]')).toHaveLength(HISTORY_PAGE + 5);
    expect(screen.queryByRole("button", { name: "Show older" })).toBeNull();
  });

  it("hides the history switch with show-history=#false", async () => {
    const env = createEnv();
    renderWithProviders(
      <NodeView
        node={widget("confirmation-center", { options: { "show-history": false } })}
        path="t"
      />,
      env,
    );
    await settle(env);
    expect(screen.queryByRole("button", { name: "History" })).toBeNull();
  });
});

describe("indicator and notices", () => {
  it("counts pending confirmations and opens the center", async () => {
    const env = createEnv();
    const stop = wire(env);
    const user = userEvent.setup();
    renderWithProviders(<NodeView node={widget("confirmation-indicator")} path="t" />, env);
    await settle(env);
    expect(document.querySelector('[data-part="confirmation-indicator"]')).toBeNull();
    await act(async () => {
      seedAgentsDemo(env.transport);
    });
    const button = await screen.findByRole("button", { name: "3 confirmations waiting" });
    expect(button).toHaveAttribute("data-risk", "destructive");
    await user.click(button);
    expect(env.store.getState().windows).toContain(CENTER_DIALOG);
    stop();
  });

  it("shows a notice per request, removes it when resolved, and Review focuses it", async () => {
    const env = createEnv();
    const stop = wire(env);
    const user = userEvent.setup();
    renderWithProviders(<ConfirmationNotices />, env);
    let send: Confirmation | undefined;
    await act(async () => {
      [send] = seedAgentsDemo(env.transport);
    });
    const notices = screen.getByRole("region", { name: "Waiting for your confirmation" });
    expect(within(notices).getAllByRole("listitem")).toHaveLength(3);
    expect(within(notices).getByText("Plug-in “Follow-up”")).toBeInTheDocument();
    // Decided elsewhere (e.g. through the agent's own prompt): the notice goes.
    await act(async () => {
      await env.client.request(
        {
          type: "DecideConfirmation",
          confirmation_id: send?.id ?? "",
          decision: "deny",
          always_allow: false,
        },
        "ConfirmationDecided",
      );
    });
    expect(within(notices).getAllByRole("listitem")).toHaveLength(2);
    const first = within(notices).getAllByRole("listitem")[0] as HTMLElement;
    await user.click(within(first).getByRole("button", { name: "Review" }));
    expect(env.store.getState().windows).toContain(CENTER_DIALOG);
    expect(agentsStoreFor(env.store).getState().focus).not.toBeNull();
    expect(within(notices).getAllByRole("listitem")).toHaveLength(1);
    await user.click(screen.getByRole("button", { name: "Hide this notice" }));
    expect(notices).not.toBeVisible();
    stop();
  });
});

describe("policy editor", () => {
  it("switches a kind, resets to the default and locks destructive kinds", async () => {
    const env = createEnv();
    const user = userEvent.setup();
    renderWithProviders(<PolicyEditor actor={CLIENT} name="Claude Desktop" />, env);
    await settle(env);
    const send = document.querySelector<HTMLElement>(
      '[data-part="action-policies.item"][data-kind="send"]',
    );
    if (!send) throw new Error("no send row");
    expect(send).toHaveAttribute("data-mode", "ask");
    await user.click(within(send).getByRole("radio", { name: "Without asking" }));
    await waitFor(() => expect(send).toHaveAttribute("data-mode", "allow"));
    expect(send).toHaveAttribute("data-customized", "");
    expect(env.transport.data.features.agents.policies["mcp-client:mcp-1"]).toEqual({
      send: "allow",
    });
    expect(screen.getByRole("status")).toHaveTextContent("Saved.");
    // Choosing the default again removes the stored mode.
    await user.click(within(send).getByRole("radio", { name: /Ask first/ }));
    await waitFor(() => expect(send).not.toHaveAttribute("data-customized"));
    expect(env.transport.data.features.agents.policies["mcp-client:mcp-1"]).toEqual({});

    const trash = document.querySelector<HTMLElement>(
      '[data-part="action-policies.item"][data-kind="trash"]',
    );
    if (!trash) throw new Error("no trash row");
    expect(trash).toHaveAttribute("data-locked", "");
    expect(within(trash).getByRole("radio", { name: "Without asking" })).toBeDisabled();
    expect(within(trash).getByText("Always asks")).toBeInTheDocument();
  });

  it("shows the core's refusal", async () => {
    const env = createEnv();
    renderWithProviders(
      <PolicyEditor actor={{ kind: "plugin", plugin_id: "ghost" }} name="Ghost" />,
      env,
    );
    expect(await screen.findByRole("alert")).toBeInTheDocument();
  });
});
