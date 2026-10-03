import { act, fireEvent, screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { emptyComposer } from "../../state/compose";
import { createEnv, renderNode, renderWithProviders, widget, type Env } from "../../test/harness";
import { NodeView } from "../../layout/LayoutRenderer";
import { WindowInstanceContext } from "../../state/windows";
import { htmlEditorIn } from "./HtmlEditor";
import { markdownViewIn } from "./MarkdownEditor";
import { composerCryptoStore } from "../security";
import { systemStoreFor } from "../system";
import { acceptRichDrafts } from "./mock";
import { AUTOSAVE_MS, richStoreFor, type RichState } from "./store";
import { loadHtmlEditor, loadMarkdownEditor } from "./lazyEditors";
import { openRichDraft } from "./flows";
import { activeComposer } from "./sessions";
import { loadInlinePreviews, missingInlineImages } from "./inlineImages";

// The editors are lazy chunks: load them once so they render at once.
beforeAll(() => Promise.all([loadHtmlEditor(), loadMarkdownEditor()]));

// jsdom has no layout: CodeMirror and ProseMirror measure text ranges.
beforeAll(() => {
  const rect = () => new DOMRect(0, 0, 0, 0);
  const rects = () => {
    const list = [] as unknown as DOMRectList;
    return Object.assign(list, { item: () => null });
  };
  Range.prototype.getBoundingClientRect = rect;
  Range.prototype.getClientRects = rects;
  if (!document.elementFromPoint) document.elementFromPoint = () => null;
});

function composerEnv(patch = {}, rich: Partial<RichState> = {}): Env {
  const env = createEnv({
    state: {
      activeAccountId: "acc-personal",
      windows: ["composer"],
      composer: { ...emptyComposer(), identityId: "ada@example.org", ...patch },
    },
  });
  acceptRichDrafts(env.transport);
  richStoreFor(env.store).setState(rich);
  return env;
}

const rich = (env: Env) => richStoreFor(env.store).getState();

describe("rich composer", () => {
  it("switches to Markdown with the core's preview", async () => {
    const user = userEvent.setup();
    const env = composerEnv({ text: "Hello *there*" });
    const { container } = renderNode(widget("composer"), env);
    const modes = screen.getByRole("radiogroup", { name: "Format" });
    expect(within(modes).getByRole("radio", { name: "Plain text" })).toBeChecked();
    await user.click(within(modes).getByRole("radio", { name: "Markdown" }));
    await waitFor(() => expect(rich(env).mode).toBe("markdown"));
    expect(screen.queryByRole("textbox", { name: "Message" })).toHaveAttribute(
      "data-part",
      "composer.markdown.content",
    );
    const view = markdownViewIn(container);
    expect(view?.state.doc.toString()).toBe("Hello *there*");
    const frame = await screen.findByTitle("Preview of the message");
    await waitFor(() =>
      expect(frame.getAttribute("srcdoc")).toContain("<p>Hello <em>there</em></p>"),
    );
    expect(frame).toHaveAttribute("sandbox", "");

    act(() => view?.dispatch({ changes: { from: 0, insert: "# Title\n\n" } }));
    expect(rich(env).markdown).toBe("# Title\n\nHello *there*");
    await waitFor(() => expect(frame.getAttribute("srcdoc")).toContain("<h1>Title</h1>"));
    expect(env.transport.requests).toContainEqual(
      expect.objectContaining({ type: "RenderPreview", identity_id: "ada@example.org" }),
    );

    await user.click(screen.getByRole("button", { name: "Preview" }));
    expect(screen.queryByTitle("Preview of the message")).toBeNull();
    await user.click(screen.getByRole("checkbox", { name: "Also send the Markdown source" }));
    expect(rich(env).markdownPart).toBe(true);
  });

  it("warns before a switch that loses formatting", async () => {
    const user = userEvent.setup();
    const env = composerEnv(
      {},
      { mode: "html", html: "<p><u>under</u> <strong>bold</strong></p>" },
    );
    renderNode(widget("composer"), env);
    await user.click(screen.getByRole("radio", { name: "Markdown" }));
    const alert = await screen.findByRole("alert");
    expect(alert).toHaveTextContent("Switching from HTML to Markdown loses formatting");
    await user.click(within(alert).getByRole("button", { name: "Keep editing" }));
    expect(rich(env).mode).toBe("html");
    expect(screen.queryByRole("alert")).toBeNull();

    await user.click(screen.getByRole("radio", { name: "Markdown" }));
    await user.click(await screen.findByRole("button", { name: "Switch anyway" }));
    expect(rich(env)).toMatchObject({ mode: "markdown", markdown: "under **bold**" });
  });

  it("edits HTML with the toolbar and the source view", async () => {
    const user = userEvent.setup();
    const env = composerEnv({}, { mode: "html", html: "<p>Hello</p>" });
    const { container } = renderNode(widget("composer"), env);
    const toolbar = screen.getByRole("toolbar", { name: "Formatting" });
    const editor = htmlEditorIn(container);
    expect(editor?.getHTML()).toBe("<p>Hello</p>");
    expect(screen.getByRole("textbox", { name: "Message" })).toHaveAttribute(
      "data-part",
      "composer.html.content",
    );
    act(() => {
      editor?.commands.selectAll();
    });
    await user.click(within(toolbar).getByRole("button", { name: "Bold" }));
    expect(rich(env).html).toBe("<p><strong>Hello</strong></p>");
    expect(within(toolbar).getByRole("button", { name: "Bold" })).toHaveAttribute(
      "aria-pressed",
      "true",
    );

    await user.click(within(toolbar).getByRole("button", { name: "Insert table" }));
    expect(rich(env).html).toContain("<table");
    expect(within(toolbar).getByRole("button", { name: "Add row" })).toBeInTheDocument();
    await user.click(within(toolbar).getByRole("button", { name: "Delete table" }));
    expect(rich(env).html).not.toContain("<table");

    await user.click(within(toolbar).getByRole("button", { name: "HTML source" }));
    const source = screen.getByRole("textbox", { name: "HTML source" });
    expect(source).toHaveValue(rich(env).html);
    fireEvent.change(source, { target: { value: "<p>From <em>source</em></p>" } });
    expect(rich(env).html).toBe("<p>From <em>source</em></p>");
    await user.click(within(toolbar).getByRole("button", { name: "HTML source" }));
    await waitFor(() => expect(editor?.getHTML()).toBe("<p>From <em>source</em></p>"));
  });

  it("adds links in the HTML editor", async () => {
    const user = userEvent.setup();
    const env = composerEnv({}, { mode: "html", html: "<p>docs</p>" });
    const { container } = renderNode(widget("composer"), env);
    act(() => {
      htmlEditorIn(container)?.commands.selectAll();
    });
    await user.click(screen.getByRole("button", { name: "Link" }));
    const group = screen.getByRole("group", { name: "Link" });
    await user.type(
      within(group).getByRole("textbox", { name: "Address" }),
      "https://nail.example",
    );
    await user.click(within(group).getByRole("button", { name: "Apply" }));
    expect(rich(env).html).toContain('href="https://nail.example"');
    expect(screen.queryByRole("group", { name: "Link" })).toBeNull();
  });

  it("inserts inline images as cid references", async () => {
    const env = composerEnv({}, { mode: "html", html: "<p>x</p>" });
    const { container } = renderNode(widget("composer"), env);
    const input = container.querySelector('[data-part="composer.html"] input[type="file"]');
    const image = new File([new Uint8Array([137, 80, 78, 71])], "logo.png", { type: "image/png" });
    fireEvent.change(input as HTMLInputElement, { target: { files: [image] } });
    await waitFor(() => expect(rich(env).attachments).toHaveLength(1));
    const cid = rich(env).attachments[0]?.content_id;
    expect(cid).toMatch(/@nail$/);
    await waitFor(() => expect(rich(env).html).toContain(`src="cid:${cid}"`));
    expect(htmlEditorIn(container)?.getHTML()).toContain('src="data:image/png;base64,');
    expect(screen.getByText("in text")).toBeInTheDocument();
  });

  it("attaches and removes files", async () => {
    const user = userEvent.setup();
    const env = composerEnv();
    const { container } = renderNode(widget("composer"), env);
    const input = container.querySelector('[data-part="composer.toolbar"] input[type="file"]');
    const file = new File(["hello"], "notes.txt", { type: "text/plain" });
    fireEvent.change(input as HTMLInputElement, { target: { files: [file] } });
    await screen.findByText("notes.txt");
    const list = screen.getByRole("list", { name: "Attachments" });
    expect(within(list).getByText("1 kB")).toBeInTheDocument();
    await user.click(within(list).getByRole("button", { name: "Remove notes.txt" }));
    expect(screen.queryByRole("list", { name: "Attachments" })).toBeNull();
    expect(rich(env).attachments).toEqual([]);
  });

  it("reminds about a missing attachment before sending", async () => {
    const user = userEvent.setup();
    const env = composerEnv({ to: "grace@navy.example", text: "The slides are attached." });
    renderNode(widget("composer"), env);
    expect(
      screen.getByText("The message mentions an attachment, but nothing is attached."),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Send" }));
    const alert = await screen.findByRole("alert");
    expect(env.transport.requests.some((r) => r.type === "SendDraft")).toBe(false);
    await user.click(within(alert).getByRole("button", { name: "Send anyway" }));
    await waitFor(() => expect(env.store.getState().composer).toBeNull());
    expect(env.transport.requests.some((r) => r.type === "SendDraft")).toBe(true);
  });

  it("sends with the sign / encrypt choice and refuses what cannot work", async () => {
    const user = userEvent.setup();
    const env = composerEnv({ to: "grace@navy.example", subject: "Protected", text: "hi" });
    renderNode(widget("composer"), env);
    const choice = { sign: true, encrypt: true, technology: null, protect_headers: null };
    // A report that leaves bob without a key blocks sending; nothing is sent.
    act(() =>
      composerCryptoStore.setState({
        choice,
        preference: "auto",
        report: {
          recipients: [
            {
              email: "bob@example.org",
              openpgp: null,
              smime: null,
              autocrypt: null,
              lookup: "not-attempted",
            },
          ],
          openpgp: { own_key: true, all_recipients: false },
          smime: { own_key: false, all_recipients: false },
          autocrypt: "disable",
          suggested: { sign: false, encrypt: false, technology: null, protect_headers: null },
        },
      }),
    );
    await user.click(screen.getByRole("button", { name: "Send" }));
    expect(await screen.findByText(/bob@example\.org/)).toBeInTheDocument();
    expect(env.transport.requests.some((r) => r.type === "SendDraft")).toBe(false);

    // Grace has a usable key: the choice goes with the draft and the send.
    act(() =>
      composerCryptoStore.setState({
        report: {
          recipients: [
            {
              email: "grace@navy.example",
              openpgp: {
                key_id: "key-3",
                fingerprint: "F",
                state: "usable",
                trust: "verified",
                source: "imported",
              },
              smime: null,
              autocrypt: null,
              lookup: "not-attempted",
            },
          ],
          openpgp: { own_key: true, all_recipients: true },
          smime: { own_key: false, all_recipients: false },
          autocrypt: "disable",
          suggested: { sign: false, encrypt: false, technology: null, protect_headers: null },
        },
      }),
    );
    await user.click(screen.getByRole("button", { name: "Send" }));
    await waitFor(() => expect(env.store.getState().composer).toBeNull());
    const saved = env.transport.requests.filter((r) => r.type === "SaveDraft").pop();
    expect(saved).toMatchObject({ draft: { crypto: choice } });
    const sent = env.transport.requests.find((r) => r.type === "SendDraft");
    expect(sent).toMatchObject({ crypto: choice });
    act(() => composerCryptoStore.getState().reset());
  });

  it("lets the reminder be dismissed", async () => {
    const user = userEvent.setup();
    const env = composerEnv({ text: "enclosed" });
    renderNode(widget("composer"), env);
    await user.click(screen.getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByText(/mentions an attachment/)).toBeNull();
  });

  it("completes addresses from the collected ones", async () => {
    const user = userEvent.setup();
    const env = composerEnv();
    renderNode(widget("composer"), env);
    const to = screen.getByRole("textbox", { name: "To" });
    await user.type(to, "bob@x.example, gr");
    const list = await screen.findByRole("listbox", { name: "Suggested addresses" });
    const option = within(list).getByRole("option", { name: /Grace Hopper/ });
    expect(option).toHaveAttribute("aria-selected", "true");
    expect(to).toHaveAttribute("aria-activedescendant", option.id);
    await user.keyboard("{Enter}");
    expect(env.store.getState().composer?.to).toBe(
      "bob@x.example, Grace Hopper <grace@navy.example>, ",
    );
    expect(screen.queryByRole("listbox")).toBeNull();
    // Enter without suggestions is not swallowed (the form would submit).
    expect(env.transport.requests.some((r) => r.type === "SendDraft")).toBe(false);

    await user.type(to, "al");
    await screen.findByRole("listbox");
    await user.keyboard("{Escape}");
    expect(screen.queryByRole("listbox")).toBeNull();
    expect(env.store.getState().windows).toEqual(["composer"]);
  });

  it("sends the Markdown message with ctrl+enter from the editor", async () => {
    const env = composerEnv(
      { to: "grace@navy.example", subject: "Plan" },
      { mode: "markdown", markdown: "# Plan" },
    );
    const { container } = renderNode(widget("composer"), env);
    const content = container.querySelector('[data-part="composer.markdown.content"]');
    fireEvent.keyDown(content as Element, { key: "Enter", ctrlKey: true });
    await waitFor(() => expect(env.store.getState().composer).toBeNull());
    const saved = env.transport.requests.find((r) => r.type === "SaveDraft");
    expect(saved).toMatchObject({ draft: { mode: "markdown", markdown: "# Plan", text: "" } });
    expect(env.store.getState().lastSend).toMatchObject({ subject: "Plan", state: "scheduled" });
  });

  it("saves with ctrl+s from the HTML editor", async () => {
    const env = composerEnv({ subject: "Keep" }, { mode: "html", html: "<p>x</p>" });
    const { container } = renderNode(widget("composer"), env);
    const content = container.querySelector('[data-part="composer.html.content"]');
    fireEvent.keyDown(content as Element, { key: "s", ctrlKey: true });
    await screen.findByText("Draft saved.");
    const id = env.store.getState().composer?.draftId ?? "";
    expect(env.transport.data.drafts.get(id)).toMatchObject({ mode: "html", html: "<p>x</p>" });
  });

  it(
    "autosaves a draft after a pause, only when something changed",
    async () => {
      const env = composerEnv({ subject: "Draft" });
      renderNode(widget("composer"), env);
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, AUTOSAVE_MS + 200));
      });
      expect(env.transport.requests.some((r) => r.type === "SaveDraft")).toBe(false);

      fireEvent.change(screen.getByRole("textbox", { name: "Subject" }), {
        target: { value: "Draft 2" },
      });
      await waitFor(
        () => expect(env.transport.requests.some((r) => r.type === "SaveDraft")).toBe(true),
        { timeout: AUTOSAVE_MS + 1500 },
      );
      await screen.findByText("Draft saved.");
      const id = env.store.getState().composer?.draftId ?? "";
      expect(env.transport.data.drafts.get(id)?.subject).toBe("Draft 2");
      expect(rich(env).autosave).toBe("saved");
      // No further save without a change.
      await act(async () => {
        await new Promise((resolve) => setTimeout(resolve, AUTOSAVE_MS + 200));
      });
      expect(env.transport.requests.filter((r) => r.type === "SaveDraft")).toHaveLength(1);
    },
    AUTOSAVE_MS * 3 + 4000,
  );

  it("switches the signature with the identity", async () => {
    const user = userEvent.setup();
    const env = composerEnv({ text: "Hi\n\n-- \nAda", identityPicked: true });
    renderNode(widget("composer"), env);
    const from = screen.getByRole("combobox", { name: "From" });
    await within(from).findByRole("option", { name: "Ada Lovelace <ada@work.example>" });
    await user.selectOptions(from, "ada@work.example");
    expect(env.store.getState().composer?.text).toBe("Hi\n\n-- \nAda Lovelace\nAnalyst");
  });
});

describe("drafts from other programs", () => {
  it("shows which program prepared the draft until dismissed", async () => {
    const user = userEvent.setup();
    const env = composerEnv({ draftId: "d-mapi", subject: "Quarterly report" });
    systemStoreFor(env.store).getState().noteProgramDraft("d-mapi", "EXCEL.EXE");
    renderNode(widget("composer"), env);
    const note = await screen.findByRole("note");
    expect(note).toHaveAttribute("data-part", "program-draft");
    expect(note).toHaveTextContent("EXCEL.EXE prepared this message");
    // The notice comes before the fields.
    const form = note.closest('[data-part="composer"]');
    expect(form?.firstElementChild).toBe(note);
    await user.click(within(note).getByRole("button", { name: "Dismiss" }));
    expect(screen.queryByRole("note")).toBeNull();
    expect(systemStoreFor(env.store).getState().programDrafts).toEqual({});
  });

  it("shows nothing for an ordinary draft", () => {
    const env = composerEnv({ draftId: "d-own" });
    renderNode(widget("composer"), env);
    expect(screen.queryByRole("note")).toBeNull();
  });
});

describe("inline images of reopened drafts", () => {
  const PNG = "iVBORw0KGgo=";

  async function savedDraft(env: Env): Promise<string> {
    const { attachment } = await env.client.request(
      {
        type: "UploadAttachment",
        filename: "logo.png",
        mime: "image/png",
        data: PNG,
        content_id: "logo1@nail",
      },
      "AttachmentUploaded",
    );
    const { draft_id } = await env.client.request(
      {
        type: "SaveDraft",
        draft: {
          id: null,
          identity_id: "ada@example.org",
          to: [],
          cc: [],
          bcc: [],
          subject: "With a logo",
          mode: "html",
          text: "",
          html: '<p>Hi</p><p><img src="cid:logo1@nail" alt="logo.png"></p>',
          markdown: null,
          attachments: [attachment],
          context: null,
          signature: "none",
          markdown_part: false,
        },
      },
      "DraftSaved",
    );
    return draft_id;
  }

  it("lists referenced inline images without a preview", () => {
    const attachment = {
      filename: "a.png",
      mime: "image/png",
      size: 1,
      blob: "b",
      content_id: "A@x",
    };
    const draft = { html: '<img src="cid:a@x">', attachments: [attachment] };
    expect(missingInlineImages(draft, {})).toEqual([attachment]);
    expect(missingInlineImages(draft, { "A@x": "data:" })).toEqual([]);
    expect(missingInlineImages({ html: "<p></p>", attachments: [attachment] }, {})).toEqual([]);
  });

  it("shows the images of a draft saved in another session", async () => {
    const env = composerEnv();
    const draftId = await savedDraft(env);
    // A new session: nothing is known about the images.
    env.store.setState({ composer: null, windows: [] });
    richStoreFor(env.store).setState({ inlinePreviews: {} });
    const ctx = {
      client: env.client,
      store: env.store,
      queryClient: env.queryClient,
      t: (id: string) => id,
      locale: "en",
    };
    await openRichDraft(ctx, draftId);
    // M5c: the draft opens in a window of its own.
    const id = activeComposer(env.store.getState().windows) ?? "";
    expect(id).toBe(`composer:${draftId}`);
    const own = () => richStoreFor(env.store, id).getState();
    await waitFor(() =>
      expect(own().inlinePreviews["logo1@nail"]).toBe(`data:image/png;base64,${PNG}`),
    );
    expect(env.transport.requests).toContainEqual(
      expect.objectContaining({ type: "GetDraftAttachment", draft_id: draftId }),
    );
    const { container } = renderWithProviders(
      <WindowInstanceContext.Provider value={id}>
        <NodeView node={widget("composer")} path="test" />
      </WindowInstanceContext.Provider>,
      env,
    );
    await waitFor(() =>
      expect(htmlEditorIn(container)?.getHTML()).toContain(`src="data:image/png;base64,${PNG}"`),
    );
    // The stored HTML still references the content id.
    expect(own().html).toContain('src="cid:logo1@nail"');
  });

  it("keeps the alt text when the core does not serve the image", async () => {
    const env = composerEnv();
    const draftId = await savedDraft(env);
    richStoreFor(env.store).setState({ inlinePreviews: {} });
    const draft = env.transport.data.drafts.get(draftId);
    if (!draft) throw new Error("no draft");
    const loaded = await loadInlinePreviews(
      { client: env.client, store: env.store },
      { ...draft, attachments: draft.attachments.map((a) => ({ ...a, blob: "unknown" })) },
    );
    expect(loaded).toBe(0);
    expect(rich(env).inlinePreviews).toEqual({});
  });
});
