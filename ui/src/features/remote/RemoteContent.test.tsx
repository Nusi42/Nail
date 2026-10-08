import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { MessageSummary, Request } from "../../api/generated";
import { createEnv, renderNode, widget, type Env } from "../../test/harness";
import { actions, handleRemoteEvent } from "./index";
import { requestsOf, wireEvents, actionContext } from "../../test/env";

/**
 * The mock's `GetMessage` fills `MessageBody.remote_content` like the core.
 * This strips it again to mimic a core that only sets
 * `remote_content_blocked`.
 */
function undecorate(env: Env) {
  const send = env.transport.send.bind(env.transport);
  env.transport.send = async (request: Request) => {
    const answer = await send(request);
    if (answer.type !== "Message") return answer;
    const message = { ...answer.message };
    delete message.remote_content;
    return { ...answer, message };
  };
}

function blockedMessage(env: Env): MessageSummary {
  const found = env.transport.data.messages.find(
    (m) => env.transport.data.bodies.get(m.id)?.remote_content_blocked === true && m.from !== null,
  );
  if (found === undefined) throw new Error("no blocked message");
  return found;
}

async function open(options: { decorated?: boolean } = {}) {
  const env = createEnv();
  wireEvents(env, handleRemoteEvent);
  if (!(options.decorated ?? true)) undecorate(env);
  const message = blockedMessage(env);
  env.store.getState().selectMessage(message.id);
  renderNode(widget("remote-content"), env);
  const bar = await screen.findByRole("region", { name: "Remote content" });
  return { env, message, bar, user: userEvent.setup() };
}

describe("remote content bar", () => {
  it("loads remote content always for the sender, using only the keyboard", async () => {
    const { env, message, bar, user } = await open();
    const sender = message.from?.email.toLowerCase() ?? "";
    const domain = sender.slice(sender.indexOf("@") + 1);
    expect(bar).toHaveTextContent("Nail blocked 3 images or other files from the internet");
    const toolbar = within(bar).getByRole("toolbar", { name: "Load remote content" });
    expect(
      within(toolbar)
        .getAllByRole("button")
        .map((b) => b.textContent),
    ).toEqual([
      "Load for this message",
      "Load once",
      `Always load from ${sender}`,
      `Always load from ${domain}`,
    ]);

    await user.tab();
    expect(within(toolbar).getByRole("button", { name: "Load for this message" })).toHaveFocus();
    await user.tab();
    await user.tab();
    await user.keyboard("{Enter}");

    await waitFor(() => expect(bar).toHaveAttribute("data-state", "allowed"));
    expect(bar).toHaveAttribute("data-scope", "sender");
    expect(bar).toHaveTextContent(`Remote content is loaded because you allowed ${sender}.`);
    expect(requestsOf(env, "AllowRemoteContent")).toEqual([
      { type: "AllowRemoteContent", message_id: message.id, scope: "sender" },
    ]);

    // …and block it again.
    await user.click(within(bar).getByRole("button", { name: "Stop loading from this sender" }));
    await waitFor(() => expect(bar).toHaveAttribute("data-state", "blocked"));
    expect(requestsOf(env, "RemoveRemoteContentAllow")).toEqual([
      { type: "RemoveRemoteContentAllow", scope: "sender", value: sender },
    ]);
  });

  it("allows one message or a domain", async () => {
    const { env, message, bar, user } = await open();
    await user.click(within(bar).getByRole("button", { name: "Load for this message" }));
    await waitFor(() => expect(bar).toHaveAttribute("data-scope", "message"));
    expect(bar).toHaveTextContent("Remote content is loaded for this message.");
    await user.click(within(bar).getByRole("button", { name: "Block again" }));
    await waitFor(() => expect(bar).toHaveAttribute("data-state", "blocked"));
    await user.click(within(bar).getByRole("button", { name: /^Always load from [^@]+$/ }));
    await waitFor(() => expect(bar).toHaveAttribute("data-scope", "domain"));
    expect(env.transport.data.features.remote.allows.map((a) => a.scope)).toEqual(["domain"]);
    expect(message.id).toBeTruthy();
  });

  it("loads once without remembering it", async () => {
    const { env, message, bar, user } = await open();
    await user.click(within(bar).getByRole("button", { name: "Load once" }));
    await waitFor(() => expect(bar).toHaveAttribute("data-scope", "once"));
    expect(bar).toHaveTextContent("Remote content is loaded this time only.");
    expect(env.transport.data.features.remote.allows).toEqual([]);
    await user.click(within(bar).getByRole("button", { name: "Block again" }));
    await waitFor(() => expect(bar).toHaveAttribute("data-state", "blocked"));
    expect(requestsOf(env, "RemoveRemoteContentAllow")).toEqual([
      { type: "RemoveRemoteContentAllow", scope: "once", value: message.id },
    ]);
  });

  it("works with a core that only says content was blocked", async () => {
    const { bar, env, user } = await open({ decorated: false });
    expect(bar).toHaveTextContent("Nail blocked images and other files from the internet");
    await user.click(within(bar).getByRole("button", { name: "Load for this message" }));
    await waitFor(() => {
      expect(env.transport.data.features.remote.allows).toHaveLength(1);
    });
  });

  it("shows errors and hides for messages without remote content", async () => {
    const { bar, env, user } = await open();
    env.transport.data.features.remote.unsupported = ["AllowRemoteContent"];
    await user.click(within(bar).getByRole("button", { name: "Load for this message" }));
    expect(await within(bar).findByRole("alert")).toHaveTextContent("This is not supported yet.");

    const plain = env.transport.data.messages.find(
      (m) => env.transport.data.bodies.get(m.id)?.remote_content_blocked === false,
    );
    act(() => {
      env.store.getState().selectMessage(plain?.id ?? null);
    });
    await waitFor(() =>
      expect(screen.queryByRole("region", { name: "Remote content" })).not.toBeInTheDocument(),
    );
  });
});

describe("remote.content.allow action", () => {
  it("is enabled only for blocked content and allows the given scope", async () => {
    const { env } = await open();
    const ctx = actionContext(env);
    const action = actions["remote.content.allow"];
    if (action === undefined) throw new Error("no action");
    expect(action.enabled?.(ctx, ["domain"])).toBe(true);
    await action.run(ctx, ["domain"]);
    expect(requestsOf(env, "AllowRemoteContent").at(-1)?.scope).toBe("domain");
    await waitFor(() => {
      expect(action.enabled?.(ctx, [])).toBe(false);
    });
    act(() => {
      env.store.getState().selectMessage(null);
    });
    expect(action.enabled?.(ctx, [])).toBe(false);
  });
});

describe("remote-content-allows (privacy settings)", () => {
  it("lists entries by scope and removes them", async () => {
    const env = createEnv();
    wireEvents(env, handleRemoteEvent);
    const message = blockedMessage(env);
    for (const scope of ["message", "sender", "domain"] as const) {
      await env.transport.send({ type: "AllowRemoteContent", message_id: message.id, scope });
    }
    const user = userEvent.setup();
    renderNode(widget("remote-content-allows"), env);
    const messages = await screen.findByRole("list", { name: "Messages" });
    expect(within(messages).getByText(message.subject)).toBeInTheDocument();
    const senders = screen.getByRole("list", { name: "Senders" });
    const sender = message.from?.email.toLowerCase() ?? "";
    await user.click(
      within(senders).getByRole("button", {
        name: `Stop loading remote content for ${sender}`,
      }),
    );
    await waitFor(() =>
      expect(screen.queryByRole("list", { name: "Senders" })).not.toBeInTheDocument(),
    );
    expect(env.transport.data.features.remote.allows.map((a) => a.scope)).toEqual([
      "message",
      "domain",
    ]);
    await user.click(within(messages).getByRole("button", { name: /Stop loading/ }));
    await user.click(
      within(screen.getByRole("list", { name: "Domains" })).getByRole("button", {
        name: /Stop loading/,
      }),
    );
    expect(await screen.findByText(/Nothing is allowed/)).toBeInTheDocument();
  });
});
