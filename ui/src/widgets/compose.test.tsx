import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { emptyComposer } from "../state/compose";
import { createEnv, renderNode, renderWithProviders, widget } from "../test/harness";
import { DiscardConfirmDialog } from "../features/composer/ConfirmDialogs";
import { identityLabel, localToIso } from "./Composer";
import { canTakeBack } from "./Outbox";

function composerEnv(patch = {}) {
  return createEnv({
    state: {
      activeAccountId: "acc-personal",
      windows: ["composer"],
      composer: { ...emptyComposer(), identityId: "ada@example.org", ...patch },
    },
  });
}

describe("composer", () => {
  it("shows the fields of the message being written", async () => {
    const env = composerEnv({ to: "grace@navy.example", subject: "Hi", text: "Body" });
    renderNode(widget("composer"), env);
    const form = screen.getByRole("form", { name: "New message" });
    expect(form).toHaveAttribute("data-key-context", "composer");
    const from = screen.getByRole("combobox", { name: "From" });
    await within(from).findByRole("option", { name: "Ada Lovelace <ada@work.example>" });
    expect(from).toHaveValue("ada@example.org");
    expect(screen.getByRole("textbox", { name: "To" })).toHaveValue("grace@navy.example");
    expect(screen.getByRole("textbox", { name: "Subject" })).toHaveValue("Hi");
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveValue("Body");
    expect(screen.queryByRole("textbox", { name: "Cc" })).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Add Cc" }));
    expect(screen.getByRole("textbox", { name: "Cc" })).toBeInTheDocument();
    fireEvent.change(screen.getByRole("textbox", { name: "Cc" }), {
      target: { value: "alan@bletchley.example" },
    });
    expect(env.store.getState().composer?.cc).toBe("alan@bletchley.example");
    fireEvent.change(from, { target: { value: "ada@work.example" } });
    expect(env.store.getState().composer).toMatchObject({
      identityId: "ada@work.example",
      identityPicked: true,
    });
  });

  it("sends with undo-send and closes", async () => {
    const user = userEvent.setup();
    const env = composerEnv({ to: "Grace <grace@navy.example>", subject: "Plans" });
    renderNode(widget("composer"), env);
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(env.store.getState().composer).toBeNull());
    expect(env.store.getState().lastSend).toMatchObject({ state: "scheduled", subject: "Plans" });
    expect(env.store.getState().windows).toEqual([]);
    expect(screen.getByText("No message is being written.")).toBeInTheDocument();
  });

  it("explains what keeps a message from being sent", async () => {
    const user = userEvent.setup();
    const env = composerEnv({ to: "grace@navy.example, nope" });
    renderNode(widget("composer"), env);
    await user.click(screen.getByRole("button", { name: "Send" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "These addresses are not valid: nope",
    );
    expect(env.transport.requests.some((r) => r.type === "SendDraft")).toBe(false);
  });

  it("schedules a message for later", async () => {
    const user = userEvent.setup();
    const env = composerEnv({ to: "grace@navy.example" });
    renderNode(widget("composer"), env);
    await user.click(screen.getByRole("button", { name: "Send later…" }));
    const at = screen.getByLabelText("Send at");
    fireEvent.change(at, { target: { value: "2031-05-01T09:30" } });
    await user.click(screen.getByRole("button", { name: "Schedule" }));
    await waitFor(() =>
      expect(env.transport.requests).toContainEqual(
        expect.objectContaining({
          type: "SendDraft",
          send_at: new Date("2031-05-01T09:30").toISOString(),
        }),
      ),
    );
  });

  it("saves and discards drafts", async () => {
    const user = userEvent.setup();
    const env = composerEnv({ subject: "Keep me" });
    renderNode(widget("composer"), env);
    await user.click(screen.getByRole("button", { name: "Save draft" }));
    expect(await screen.findByText("Draft saved.")).toBeInTheDocument();
    const id = env.store.getState().composer?.draftId ?? "";
    expect(env.transport.data.drafts.get(id)?.subject).toBe("Keep me");
    await user.click(screen.getByRole("button", { name: "Discard" }));
    // A saved draft asks first (M5c: a modal confirmation window).
    expect(env.store.getState().windows).toEqual(["composer", "composer-discard"]);
    const confirm = renderWithProviders(
      <DiscardConfirmDialog
        name="composer-discard"
        close={() => env.store.getState().closeWindow("composer-discard")}
      />,
      env,
    );
    await user.click(within(confirm.container).getByRole("button", { name: "Discard" }));
    await waitFor(() => expect(env.transport.data.drafts.has(id)).toBe(false));
    expect(env.store.getState().composer).toBeNull();
  });

  it("follows a virtual account's identity rules until an identity is picked", async () => {
    const env = composerEnv({ identityId: "ada@example.org" });
    env.store.setState({ activeAccountId: "team" });
    env.transport.data.accountsConfig.virtual_accounts.push({
      id: "team",
      name: "Team",
      members: ["acc-personal", "acc-work"],
      merge_roles: true,
      folder_mappings: [],
      default_identity: null,
      identity_rules: [{ pattern: "*@client.example", identity: "ada@work.example" }],
    });
    renderNode(widget("composer"), env);
    fireEvent.change(screen.getByRole("textbox", { name: "To" }), {
      target: { value: "boss@client.example" },
    });
    await waitFor(() => expect(env.store.getState().composer?.identityId).toBe("ada@work.example"));
    act(() =>
      env.store.getState().updateComposer({ identityId: "ada@example.org", identityPicked: true }),
    );
    fireEvent.change(screen.getByRole("textbox", { name: "To" }), {
      target: { value: "other@client.example" },
    });
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 20));
    });
    expect(env.store.getState().composer?.identityId).toBe("ada@example.org");
  });

  it("formats identities and send-later times", () => {
    expect(
      identityLabel({
        id: "x",
        account_id: "a",
        email: "x@y.z",
        name: null,
        reply_to: null,
        bcc: [],
        signature: null,
        composer_mode: null,
        posting: null,
        mail_style: null,
      }),
    ).toBe("x@y.z");
    expect(localToIso("")).toBeNull();
    expect(localToIso("garbage")).toBeNull();
    expect(localToIso("2031-05-01T09:30")).toBe(new Date("2031-05-01T09:30").toISOString());
  });
});

describe("outbox", () => {
  async function queue(
    env: ReturnType<typeof createEnv>,
    delay: number | null,
    sendAt: string | null,
  ) {
    const { draft_id } = await env.client.request(
      {
        type: "SaveDraft",
        draft: {
          id: null,
          identity_id: "ada@example.org",
          to: [{ name: "Grace", email: "grace@navy.example" }],
          cc: [],
          bcc: [],
          subject: "Later",
          mode: "plain",
          text: "x",
          html: null,
          markdown: null,
          attachments: [],
          context: null,
          signature: "identity",
          markdown_part: false,
        },
      },
      "DraftSaved",
    );
    return (
      await env.client.request(
        { type: "SendDraft", draft_id, delay_secs: delay, send_at: sendAt },
        "SendQueued",
      )
    ).entry;
  }

  it("lists waiting messages and takes one back into the composer", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    await queue(env, null, new Date(Date.now() + 86_400_000).toISOString());
    const { container } = renderNode(widget("outbox"), env);
    const item = (await screen.findByText("Later")).closest(
      '[data-part="outbox.item"]',
    ) as HTMLElement;
    expect(item).toHaveAttribute("data-state", "scheduled");
    expect(within(item).getByText("To: Grace")).toBeInTheDocument();
    expect(within(item).getByText("Waiting")).toBeInTheDocument();
    await user.click(within(item).getByRole("button", { name: "Edit" }));
    await waitFor(() => expect(env.store.getState().composer?.subject).toBe("Later"));
    expect(env.store.getState().windows.some((w) => w.startsWith("composer:"))).toBe(true);
    expect(await screen.findByText("Nothing is waiting to be sent.")).toBeInTheDocument();
    // It is a saved draft again.
    await waitFor(() =>
      expect(container.querySelectorAll('[data-part="outbox.item"]')).toHaveLength(1),
    );
    expect(container.querySelector('[data-part="outbox.item"]')).toHaveAttribute(
      "data-state",
      "draft",
    );
  });

  it("lists saved drafts to continue or discard", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    const save = (subject: string) =>
      env.client.request(
        {
          type: "SaveDraft",
          draft: {
            id: null,
            identity_id: "ada@example.org",
            to: [{ name: "Grace", email: "grace@navy.example" }],
            cc: [],
            bcc: [],
            subject,
            mode: "markdown",
            text: "",
            html: null,
            markdown: "**draft**",
            attachments: [],
            context: null,
            signature: "identity",
            markdown_part: false,
          },
        },
        "DraftSaved",
      );
    await save("Unfinished");
    await save("Abandoned");
    renderNode(widget("outbox"), env);
    const drafts = await screen.findByRole("region", { name: "Drafts" });
    const abandoned = within(drafts).getByText("Abandoned").closest("li") as HTMLElement;
    await user.click(within(abandoned).getByRole("button", { name: "Discard" }));
    await waitFor(() => expect(within(drafts).queryByText("Abandoned")).toBeNull());
    const unfinished = within(drafts).getByText("Unfinished").closest("li") as HTMLElement;
    await user.click(within(unfinished).getByRole("button", { name: "Continue writing" }));
    await waitFor(() => expect(env.store.getState().composer?.subject).toBe("Unfinished"));
    expect(env.store.getState().windows.some((w) => w.startsWith("composer:"))).toBe(true);
  });

  it("sends a waiting message at once", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    await queue(env, 3600, null);
    renderNode(widget("outbox"), env);
    await user.click(await screen.findByRole("button", { name: "Send now" }));
    await waitFor(() =>
      expect(env.transport.requests).toContainEqual(
        expect.objectContaining({ type: "SendDraft", delay_secs: 0 }),
      ),
    );
    expect(env.store.getState().lastSend?.state).toBe("scheduled");
  });

  it("shows failures", async () => {
    const env = createEnv();
    env.transport.data.behaviour.sendError = "rejected";
    await queue(env, 0, null);
    await waitFor(() => expect(env.transport.data.outbox[0]?.state).toBe("failed"));
    renderNode(widget("outbox"), env);
    expect(await screen.findByText("The server refused the request.")).toHaveAttribute(
      "data-part",
      "outbox.item.error",
    );
    expect(canTakeBack({ state: "failed" } as Parameters<typeof canTakeBack>[0])).toBe(true);
    expect(canTakeBack({ state: "sending" } as Parameters<typeof canTakeBack>[0])).toBe(false);
  });
});

describe("status-bar send status", () => {
  it("counts down, offers undo, and reports the result", async () => {
    const user = userEvent.setup();
    const env = createEnv();
    renderNode(widget("status-bar", { options: { items: "send-status" } }), env);
    const status = screen.getByRole("status", { name: "Status" });
    const { draft_id } = await env.client.request(
      {
        type: "SaveDraft",
        draft: {
          id: null,
          identity_id: "ada@example.org",
          to: [{ name: null, email: "grace@navy.example" }],
          cc: [],
          bcc: [],
          subject: "Undo me",
          mode: "plain",
          text: "",
          html: null,
          markdown: null,
          attachments: [],
          context: null,
          signature: "identity",
          markdown_part: false,
        },
      },
      "DraftSaved",
    );
    const { entry } = await env.client.request(
      { type: "SendDraft", draft_id, delay_secs: 8, send_at: null },
      "SendQueued",
    );
    act(() => env.store.getState().setLastSend(entry));
    expect(within(status).getByText(/Sending in [78] seconds/)).toBeInTheDocument();
    await user.click(within(status).getByRole("button", { name: "Undo" }));
    await waitFor(() => expect(env.store.getState().composer?.subject).toBe("Undo me"));
    expect(env.store.getState().lastSend).toBeNull();

    act(() => env.store.getState().setLastSend({ ...entry, state: "failed", error: "too_large" }));
    expect(
      within(status).getByText("Sending failed: The message is larger than the server accepts."),
    ).toBeInTheDocument();
    act(() => env.store.getState().setLastSend({ ...entry, state: "sent" }));
    expect(within(status).getByText("Message sent.")).toBeInTheDocument();
    act(() =>
      env.store.getState().setLastSend({
        ...entry,
        state: "scheduled",
        send_at: new Date(Date.now() + 86_400_000).toISOString(),
      }),
    );
    expect(within(status).getByText(/Scheduled to send/)).toBeInTheDocument();
  });
});
