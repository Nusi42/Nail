/**
 * The M4a extension UI against the mock core with the demo plug-ins:
 * banners / badges, attachment previews and action menus, composer
 * extensions and edits, the before-send veto, and the plug-in manager's
 * activity and confirmation sections.
 */
import { act, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { useCallback, useState } from "react";
import { vi } from "vitest";
import type { AttachmentInfo, OutboxEntry } from "../../api/generated";
import { createEnv, renderWithProviders, type Env } from "../../test/harness";
import {
  applyComposerEdit,
  AttachmentActionMenu,
  AttachmentPreviewPane,
  ComposerEditNotices,
  ComposerExtensionBar,
  LinkActionMenu,
  PluginBadges,
  PluginBanners,
  PluginMarkdownBlock,
  sendDraftChecked,
  SendVetoDialog,
  useComposerEdits,
} from ".";
import { answerSendDraft } from "./extensionsMock";
import { installDemoExtensions } from "./extensionsSeed";
import { PluginManagerDialog } from "./PluginManager";
import { PluginNotifications } from "./PluginNotifications";
import { pluginsStoreFor } from "./store";
import { tick, wireEvents } from "./testUtil";
import { WindowLayer } from "../../layout/WindowLayer";

function setup(): Env {
  const env = createEnv();
  env.transport.data.behaviour.workMs = 0;
  installDemoExtensions(env.transport);
  return env;
}

async function settle(env: Env) {
  await waitFor(() => expect(env.queryClient.isFetching()).toBe(0));
  await act(() => tick());
}

function withAttachments(env: Env): {
  messageId: string;
  pdf: AttachmentInfo;
  csv: AttachmentInfo;
} {
  const m = env.transport.data.messages.find(
    (x) => (env.transport.data.bodies.get(x.id)?.attachments.length ?? 0) > 1,
  );
  const body = m ? env.transport.data.bodies.get(m.id) : undefined;
  if (!m || !body?.attachments[0] || !body.attachments[1]) throw new Error("no attachments");
  return { messageId: m.id, pdf: body.attachments[0], csv: body.attachments[1] };
}

describe("reader: banners and badges", () => {
  it("names the plug-in and sends banner buttons back to it", async () => {
    const env = setup();
    const stop = wireEvents(env);
    const user = userEvent.setup();
    const id = env.transport.data.messages[0]?.id ?? "";
    renderWithProviders(
      <>
        <PluginBadges messageId={id} />
        <PluginBanners messageId={id} />
        <PluginNotifications />
      </>,
      env,
    );
    await settle(env);
    const banners = screen.getByRole("list", { name: "Notes from plug-ins" });
    const banner = within(banners).getByRole("listitem");
    expect(banner).toHaveAttribute("data-plugin", "link-guard");
    expect(banner).toHaveAttribute("data-level", "warning");
    expect(within(banner).getByText("Link guard:")).toBeInTheDocument();
    const badges = screen.getByRole("list", { name: "Plug-in badges" });
    expect(within(badges).getByText("External")).toBeInTheDocument();
    expect(
      within(badges).getByText("Sent from outside example.com (added by the plug-in Link guard)"),
    ).toBeInTheDocument();
    await user.click(within(banner).getByRole("button", { name: "Report as phishing" }));
    await settle(env);
    expect(await screen.findByText("Reported as phishing")).toBeInTheDocument();
    stop();
  });

  it("shows nothing when no plug-in annotates the message", async () => {
    const env = createEnv();
    const id = env.transport.data.messages[0]?.id ?? "";
    const { container } = renderWithProviders(
      <>
        <PluginBadges messageId={id} />
        <PluginBanners messageId={id} />
      </>,
      env,
    );
    await settle(env);
    expect(container).toBeEmptyDOMElement();
  });
});

describe("reader: attachments and links", () => {
  it("previews a CSV as a table and a PDF as an image, naming the plug-in", async () => {
    const env = setup();
    const { messageId, pdf, csv } = withAttachments(env);
    renderWithProviders(
      <>
        <AttachmentPreviewPane messageId={messageId} attachment={csv} />
        <AttachmentPreviewPane messageId={messageId} attachment={pdf} />
      </>,
      env,
    );
    await settle(env);
    const table = screen.getByRole("region", { name: "Preview of q3-figures.csv" });
    expect(within(table).getByText("Table, shown by the plug-in File preview")).toBeInTheDocument();
    expect(within(table).getByRole("table")).toBeInTheDocument();
    expect(within(table).getByText("1.5 M")).toBeInTheDocument();
    const image = screen.getByRole("region", { name: "Preview of report.pdf" });
    const img = within(image).getByRole("img", { name: "First page of report.pdf" });
    expect(img.getAttribute("src")).toMatch(/^data:image\/png;base64,/);
  });

  it("lets the user choose between renderers and shows refusals", async () => {
    const env = setup();
    const { messageId, csv } = withAttachments(env);
    const plugins = env.transport.data.features.plugins;
    const preview = plugins.plugins.find((p) => p.id === "file-preview");
    if (!preview?.manifest.contributes?.attachmentPreviews) throw new Error("no previews");
    preview.manifest.contributes.attachmentPreviews.push({
      id: "raw",
      title: "Raw text",
      extensions: ["csv"],
    });
    const user = userEvent.setup();
    renderWithProviders(<AttachmentPreviewPane messageId={messageId} attachment={csv} />, env);
    await settle(env);
    // The "raw" renderer has no behaviour: the core's refusal is shown.
    await user.selectOptions(
      screen.getByRole("combobox", { name: "Preview with" }),
      "file-preview.raw",
    );
    await settle(env);
    expect(screen.getByRole("alert")).toBeInTheDocument();
  });

  it("offers nothing for an attachment no plug-in previews", async () => {
    const env = createEnv();
    const att: AttachmentInfo = {
      index: 0,
      filename: "a.zip",
      mime: "application/zip",
      size: 1,
      inline: false,
    };
    const { container } = renderWithProviders(
      <AttachmentPreviewPane messageId="m" attachment={att} />,
      env,
    );
    await settle(env);
    expect(container).toBeEmptyDOMElement();
  });

  it("runs attachment and link actions from keyboard-operable menus", async () => {
    const env = setup();
    const stop = wireEvents(env);
    const { messageId, csv } = withAttachments(env);
    const user = userEvent.setup();
    renderWithProviders(
      <>
        <AttachmentActionMenu messageId={messageId} attachment={csv} />
        <LinkActionMenu url="https://docs.example.org/a" text="docs" messageId={messageId} />
        <PluginNotifications />
      </>,
      env,
    );
    await settle(env);
    const button = screen.getByRole("button", { name: "Plug-in actions for q3-figures.csv" });
    expect(button).toHaveAttribute("aria-haspopup", "menu");
    button.focus();
    await user.keyboard("{ArrowDown}");
    const item = screen.getByRole("menuitem", { name: /Show file summary/ });
    expect(item).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("menu")).toBeNull();
    expect(button).toHaveFocus();
    await user.click(button);
    await user.click(screen.getByRole("menuitem", { name: /Show file summary/ }));
    await settle(env);
    expect(await screen.findByText("3 rows, 3 columns")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Plug-in actions for this link" }));
    const link = screen.getByRole("menuitem", { name: /Check link reputation/ });
    expect(within(link).getByText("Link guard")).toBeInTheDocument();
    await user.click(link);
    await settle(env);
    expect(await screen.findByText("Link looks safe")).toBeInTheDocument();
    stop();
  });
});

function TestComposer({ draftId }: { draftId: string }) {
  const [text, setText] = useState("hello there");
  const [selection, setSelection] = useState({ start: 0, end: 5 });
  const apply = useCallback(
    (edit: { edit: { op: "insert" | "replace-selection" | "replace-all"; text: string } }) => {
      setText((current) => {
        const next = applyComposerEdit(current, selection, edit.edit);
        setSelection({ start: next.cursor, end: next.cursor });
        return next.text;
      });
    },
    [selection],
  );
  useComposerEdits(draftId, apply);
  const [inserted, setInserted] = useState("");
  return (
    <>
      <ComposerExtensionBar
        draftId={draftId}
        getSelection={() => ({ text: text.slice(selection.start, selection.end), ...selection })}
        onInsert={setInserted}
      />
      <div data-testid="text">{text}</div>
      <div data-testid="inserted">{inserted}</div>
      <ComposerEditNotices draftId={draftId} />
    </>
  );
}

describe("composer extensions", () => {
  it("runs composer actions whose edits the composer applies, and inserts blocks", async () => {
    const env = setup();
    const stop = wireEvents(env);
    const user = userEvent.setup();
    renderWithProviders(<TestComposer draftId="d1" />, env);
    await settle(env);
    await user.click(screen.getByRole("button", { name: "Plug-in actions" }));
    await user.click(screen.getByRole("menuitem", { name: /Make selection uppercase/ }));
    await settle(env);
    await waitFor(() => expect(screen.getByTestId("text")).toHaveTextContent("HELLO there"));
    expect(screen.getByRole("status")).toHaveTextContent(
      "Plug-in “Snippets” replaced the selected text.",
    );
    expect(pluginsStoreFor(env.store).getState().edits).toEqual([]);
    await user.click(screen.getByRole("button", { name: "Insert block" }));
    await user.click(screen.getByRole("menuitem", { name: /Ticket card/ }));
    expect(screen.getByTestId("inserted").textContent).toBe(
      "```snippets.ticket\nid: NAIL-123\nstatus: open\n```\n",
    );
    stop();
  });

  it("keeps edits of other drafts queued and offers nothing without plug-ins", async () => {
    const env = createEnv();
    const stop = wireEvents(env);
    const { container } = renderWithProviders(<TestComposer draftId="d1" />, env);
    await settle(env);
    expect(screen.queryByRole("button", { name: "Plug-in actions" })).toBeNull();
    act(() =>
      env.transport.emit({
        type: "ComposerEditRequested",
        draft_id: "other",
        client_id: null,
        by: { actor: { kind: "mcp-client", client_id: "mcp-1" }, name: "Claude Desktop" },
        edit: { op: "replace-all", text: "x" },
      }),
    );
    expect(pluginsStoreFor(env.store).getState().edits).toHaveLength(1);
    act(() =>
      env.transport.emit({
        type: "ComposerEditRequested",
        draft_id: "d1",
        client_id: null,
        by: { actor: { kind: "mcp-client", client_id: "mcp-1" }, name: "Claude Desktop" },
        edit: { op: "replace-all", text: "Rewritten" },
      }),
    );
    await waitFor(() => expect(screen.getByTestId("text")).toHaveTextContent("Rewritten"));
    expect(within(container).getByRole("status")).toHaveTextContent(
      "AI agent “Claude Desktop” (MCP client) replaced the whole text.",
    );
    // Edits for another UI are not applied here.
    act(() =>
      env.transport.emit({
        type: "ComposerEditRequested",
        draft_id: "d1",
        client_id: "another-ui",
        by: { actor: { kind: "plugin", plugin_id: "snippets" }, name: "Snippets" },
        edit: { op: "replace-all", text: "nope" },
      }),
    );
    await act(() => tick());
    expect(screen.getByTestId("text")).toHaveTextContent("Rewritten");
    stop();
  });

  it("draws plug-in Markdown blocks in the preview with who renders them", async () => {
    const env = setup();
    renderWithProviders(
      <>
        <PluginMarkdownBlock info="snippets.ticket" body={"id: NAIL-1"} />
        <PluginMarkdownBlock info="acme.unknown" body="x" />
      </>,
      env,
    );
    await settle(env);
    expect(
      screen.getByText("Ticket card: rendered by the plug-in Snippets when the message is sent"),
    ).toBeInTheDocument();
    expect(
      screen.getByText("No plug-in renders “acme.unknown”; it is sent as a code block."),
    ).toBeInTheDocument();
  });
});

describe("before-send veto", () => {
  async function vetoedDraft(env: Env): Promise<string> {
    const r = await env.transport.send({
      type: "SaveDraft",
      draft: {
        id: null,
        identity_id: "ada@work.example",
        to: [{ name: null, email: "grace@example.org" }],
        cc: [],
        bcc: [],
        subject: "Figures",
        mode: "plain",
        text: "The figures are attached.",
        html: null,
        markdown: null,
        attachments: [],
        context: null,
        signature: "none",
        markdown_part: false,
      },
    });
    if (r.type !== "DraftSaved") throw new Error("not saved");
    const original = env.transport.send.bind(env.transport);
    vi.spyOn(env.transport, "send").mockImplementation((request) =>
      request.type === "SendDraft"
        ? Promise.resolve(answerSendDraft(env.transport, request))
        : original(request),
    );
    return r.draft_id;
  }

  function send(env: Env, draftId: string): Promise<OutboxEntry | null> {
    return sendDraftChecked(env.client, env.store, {
      type: "SendDraft",
      draft_id: draftId,
      delay_secs: 0,
      send_at: null,
    });
  }

  it("names the plug-in and its reason; Keep editing sends nothing", async () => {
    const env = setup();
    const id = await vetoedDraft(env);
    const user = userEvent.setup();
    renderWithProviders(
      <>
        <SendVetoDialog />
        <WindowLayer layout={env.transport.data.layout} />
      </>,
      env,
    );
    let result: Promise<OutboxEntry | null> | undefined;
    await act(async () => {
      result = send(env, id);
      await tick();
    });
    const dialog = screen.getByRole("dialog", { name: "A plug-in stopped this message" });
    expect(within(dialog).getByText("Plug-in “Attachment check”:")).toBeInTheDocument();
    expect(
      within(dialog).getByText("The message mentions an attachment, but nothing is attached."),
    ).toBeInTheDocument();
    const keep = within(dialog).getByRole("button", { name: "Keep editing" });
    expect(keep).toHaveFocus();
    await user.keyboard("{Escape}");
    expect(await result).toBeNull();
    expect(screen.queryByRole("dialog")).toBeNull();
    expect(env.transport.data.outbox).toHaveLength(0);
  });

  it("sends anyway on request", async () => {
    const env = setup();
    const id = await vetoedDraft(env);
    const user = userEvent.setup();
    renderWithProviders(
      <>
        <SendVetoDialog />
        <WindowLayer layout={env.transport.data.layout} />
      </>,
      env,
    );
    let result: Promise<OutboxEntry | null> | undefined;
    await act(async () => {
      result = send(env, id);
      await tick();
    });
    await user.click(screen.getByRole("button", { name: "Send anyway" }));
    const entry = await result;
    expect(entry).not.toBeNull();
    expect(entry?.subject).toBe("Figures");
    expect(screen.queryByRole("dialog")).toBeNull();
  });

  it("explains an expired veto and offers only Keep editing", async () => {
    const env = setup();
    const id = await vetoedDraft(env);
    const user = userEvent.setup();
    renderWithProviders(
      <>
        <SendVetoDialog />
        <WindowLayer layout={env.transport.data.layout} />
      </>,
      env,
    );
    let result: Promise<OutboxEntry | null> | undefined;
    await act(async () => {
      result = send(env, id);
      await tick();
    });
    env.transport.data.features.plugins.extensions.vetoes.clear();
    await user.click(screen.getByRole("button", { name: "Send anyway" }));
    expect(
      await screen.findByText(
        "This was too long ago. Send the message again to check it once more.",
      ),
    ).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Send anyway" })).toBeNull();
    await user.click(screen.getByRole("button", { name: "Keep editing" }));
    expect(await result).toBeNull();
  });

  it("passes through queued sends and errors", async () => {
    const env = createEnv();
    await expect(send(env, "nope")).rejects.toMatchObject({ code: "not_found" });
  });
});

describe("plug-in manager additions", () => {
  it("shows hooks, jobs with last / next run and CPU, and runs a job", async () => {
    const env = setup();
    const stop = wireEvents(env);
    const user = userEvent.setup();
    renderWithProviders(<PluginManagerDialog />, env);
    await settle(env);
    const digest = document.querySelector<HTMLElement>('[data-plugin="digest"]');
    if (!digest) throw new Error("no digest row");
    // Digest cannot write: no confirmations section.
    expect(within(digest).queryByRole("button", { name: "Confirmations" })).toBeNull();
    await user.click(within(digest).getByRole("button", { name: "Activity" }));
    await settle(env);
    const meter = within(digest).getByRole("meter");
    expect(meter).toHaveAttribute("aria-valuetext", "18.4 s of 120 s (15 %)");
    const morning = digest.querySelector<HTMLElement>('[data-job="morning"]');
    if (!morning) throw new Error("no job");
    expect(within(morning).getByText("0 8 * * 1-5")).toBeInTheDocument();
    expect(within(morning).getByText("14, 1 failed")).toBeInTheDocument();
    const count = digest.querySelector<HTMLElement>('[data-job="count"]');
    expect(count).toHaveAttribute("data-state", "timeout");
    expect(within(count as HTMLElement).getByText("message.received")).toBeInTheDocument();
    await user.click(within(morning).getByRole("button", { name: "Run “Morning digest” now" }));
    await settle(env);
    await waitFor(() => expect(within(morning).getByText("15, 1 failed")).toBeInTheDocument());
    expect(within(digest).getByText("Tool for AI agents: digest__digest")).toBeInTheDocument();

    const check = document.querySelector<HTMLElement>('[data-plugin="attachment-check"]');
    if (!check) throw new Error("no attachment-check row");
    await user.click(within(check).getByRole("button", { name: "Activity" }));
    await settle(env);
    const hooks = within(check).getByRole("table", { name: "Mail hooks" });
    const row = within(hooks).getByRole("row", { name: /Before sending/ });
    expect(within(row).getByText("40")).toBeInTheDocument();
    expect(within(check).getByText("Mail hook: Before sending")).toBeInTheDocument();
    stop();
  });

  it("offers the confirmation policy for plug-ins that may write, and highlights writes", async () => {
    const env = setup();
    const writer = env.transport.data.features.plugins.plugins.find((p) => p.id === "snippets");
    if (!writer) throw new Error("no snippets");
    writer.manifest.permissions = ["composer", "tasks.write"];
    writer.grants = [{ permission: "composer", accounts: null }];
    const user = userEvent.setup();
    renderWithProviders(<PluginManagerDialog />, env);
    await settle(env);
    const row = document.querySelector<HTMLElement>('[data-plugin="snippets"]');
    if (!row) throw new Error("no row");
    await user.click(within(row).getByRole("button", { name: "Confirmations" }));
    await settle(env);
    expect(
      within(row).getByRole("region", { name: "What Snippets may do without asking" }),
    ).toBeInTheDocument();
    await user.click(within(row).getByRole("button", { name: "Permissions" }));
    const write = row.querySelector('[data-permission="tasks.write"]');
    expect(write).toHaveAttribute("data-write", "");
    expect(within(write as HTMLElement).getByText("Can change your data")).toBeInTheDocument();
  });
});
