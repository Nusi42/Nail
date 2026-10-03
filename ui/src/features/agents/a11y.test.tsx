/**
 * Accessibility of the confirmation center (axe-core in jsdom, like
 * `src/a11y.test.tsx`): the widget with pending and decided
 * confirmations, the dialog, the indicator, the notices, the policy
 * editor, and the MCP client settings with their M4a parts. Contrast
 * needs a real layout engine (Playwright).
 */
import { act, screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { violations } from "../../test/axe";
import { NodeView } from "../../layout/LayoutRenderer";
import { createEnv, renderWithProviders, widget, type Env } from "../../test/harness";
import { handleAgentsEvent, PolicyEditor, seedAgentsDemo } from ".";
import { ConfirmationCenterDialog } from "./ConfirmationCenter";
import { ConfirmationNotices } from "./ConfirmationNotices";

async function settle(env: Env): Promise<void> {
  await waitFor(() => expect(env.queryClient.isFetching()).toBe(0));
  await act(async () => {
    await new Promise((resolve) => setTimeout(resolve, 20));
  });
}

describe("axe: confirmation center", () => {
  it("the widget (pending and history), indicator and policy editor have no violations", async () => {
    const env = createEnv();
    seedAgentsDemo(env.transport);
    const user = userEvent.setup();
    const { container } = renderWithProviders(
      <>
        <NodeView node={widget("confirmation-indicator")} path="t/0" />
        <NodeView node={widget("confirmation-center")} path="t/1" />
        <PolicyEditor actor={{ kind: "mcp-client", client_id: "mcp-1" }} name="Claude Desktop" />
      </>,
      env,
    );
    await settle(env);
    expect(await violations(container)).toEqual([]);
    await user.click(screen.getByRole("button", { name: "History" }));
    await settle(env);
    expect(await violations(container)).toEqual([]);
  });

  it("the dialog and the notices have no violations", async () => {
    const env = createEnv();
    const stop = env.client.subscribe((event) =>
      handleAgentsEvent(event, {
        client: env.client,
        queryClient: env.queryClient,
        store: env.store,
      }),
    );
    const { container } = renderWithProviders(
      <>
        <ConfirmationCenterDialog />
        <ConfirmationNotices />
      </>,
      env,
    );
    await act(async () => {
      seedAgentsDemo(env.transport);
    });
    await settle(env);
    expect(screen.getAllByRole("button", { name: "Review" }).length).toBeGreaterThan(0);
    expect(await violations(container)).toEqual([]);
    stop();
  });

  it("the MCP client settings with writing scopes, limits and policy have no violations", async () => {
    const env = createEnv();
    seedAgentsDemo(env.transport);
    const { container } = renderWithProviders(
      <NodeView node={widget("mcp-clients")} path="t/0" />,
      env,
    );
    await settle(env);
    expect(screen.getByRole("form", { name: "Limits for Claude Desktop" })).toBeInTheDocument();
    expect(await violations(container)).toEqual([]);
  });
});
