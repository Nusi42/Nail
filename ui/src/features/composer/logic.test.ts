import { QueryClient } from "@tanstack/react-query";
import { TransportClient } from "../../api/client";
import {
  MAX_ATTACHMENT_BYTES,
  type Draft,
  type Identity,
  type Response,
} from "../../api/generated";
import { MockTransport } from "../../api/mock";
import { createMockData } from "../../api/mockData";
import { emptyComposer, type ComposeContext } from "../../state/compose";
import { createUiStore } from "../../state/uiStore";
import { completeEntry, currentEntry } from "./AddressField";
import {
  bodyOf,
  draftKey,
  fromDraft,
  referencedCids,
  richDraftOf,
  toDisplayHtml,
  toStoredHtml,
} from "./draft";
import {
  cancelSwitch,
  confirmSwitch,
  openRichDraft,
  saveRich,
  sendRich,
  startRichCompose,
  startRichReply,
  swapSignature,
  switchMode,
  uploadFiles,
} from "./flows";
import { acceptRichDrafts, convertDraft, renderMarkdown, saveRichDraft, suggest } from "./mock";
import { mentionsAttachment, needsReminder, ownText, parseKeywords } from "./reminder";
import { activeComposer } from "./sessions";
import { emptyRich, instanceRichStore, primeRich, richStoreFor } from "./store";
import { must } from "../../test/must";

/** The rich store of the focused composer window. */
const activeRich = (ctx: ComposeContext) =>
  richStoreFor(ctx.store, activeComposer(ctx.store.getState().windows));

const KEYWORDS = "attach, attached, attachment, enclosed";

function context(): ComposeContext & { transport: MockTransport } {
  const transport = new MockTransport({ data: createMockData(10) });
  acceptRichDrafts(transport);
  return {
    transport,
    client: new TransportClient(transport),
    store: createUiStore({ connection: "connected", activeAccountId: "acc-personal" }),
    queryClient: new QueryClient(),
    t: (id, args) =>
      id === "composer-attachment-keywords" ? KEYWORDS : `${id}${args ? JSON.stringify(args) : ""}`,
    locale: "en",
  };
}

function firstMessage(transport: MockTransport) {
  const message = transport.data.messages[0];
  if (!message) throw new Error("no mock messages");
  return message;
}

function firstIdentity(transport: MockTransport): Identity {
  const identity = transport.data.accountsConfig.accounts[0]?.identities[0];
  if (!identity) throw new Error("no mock identity");
  return identity;
}

function draft(patch: Partial<Draft> = {}): Draft {
  return {
    id: null,
    identity_id: "ada@example.org",
    to: [{ name: "Grace", email: "grace@navy.example" }],
    cc: [],
    bcc: [],
    subject: "S",
    mode: "plain",
    text: "",
    html: null,
    markdown: null,
    attachments: [],
    context: null,
    signature: "identity",
    markdown_part: false,
    ...patch,
  };
}

describe("draft mapping", () => {
  it("builds drafts in every mode and round-trips them", () => {
    const composer = { ...emptyComposer(), identityId: "ada@example.org", to: "a@b.example" };
    const rich = {
      ...emptyRich(),
      mode: "markdown" as const,
      markdown: "# Hi",
      markdownPart: true,
    };
    const { draft: d, invalid } = richDraftOf({ ...composer, text: "ignored" }, rich);
    expect(invalid).toEqual([]);
    expect(d).toMatchObject({ mode: "markdown", markdown: "# Hi", html: null, text: "" });
    expect(d.markdown_part).toBe(true);
    const back = fromDraft({ ...d, id: "draft-1" });
    expect(back.composer).toMatchObject({ draftId: "draft-1", to: "a@b.example", text: "" });
    expect(back.rich).toMatchObject({ mode: "markdown", markdown: "# Hi", markdownPart: true });
    expect(bodyOf(back.composer, { ...emptyRich(), ...back.rich })).toBe("# Hi");

    const html = richDraftOf(composer, { ...rich, mode: "html", html: "<p>x</p>" }).draft;
    expect(html).toMatchObject({ html: "<p>x</p>", markdown: null, markdown_part: false });
    const plain = richDraftOf({ ...composer, text: "t" }, emptyRich()).draft;
    expect(plain).toMatchObject({ mode: "plain", text: "t", html: null, markdown: null });
  });

  it("keys change with content, not with the draft id", () => {
    const composer = { ...emptyComposer(), identityId: "x" };
    const rich = emptyRich();
    expect(draftKey({ ...composer, draftId: "draft-9" }, rich)).toBe(draftKey(composer, rich));
    expect(draftKey({ ...composer, subject: "s" }, rich)).not.toBe(draftKey(composer, rich));
  });

  it("maps inline image previews to cid references and back", () => {
    const previews = { "logo@nail": "data:image/png;base64,AAAA" };
    const stored = '<p><img src="cid:logo@nail" alt="Logo"></p>';
    const shown = toDisplayHtml(stored, previews);
    expect(shown).toContain('src="data:image/png;base64,AAAA"');
    expect(toStoredHtml(shown, previews)).toBe(stored);
    expect([...referencedCids(stored)]).toEqual(["logo@nail"]);
  });
});

describe("attachment reminder", () => {
  const keywords = parseKeywords(" Attached,  enclosed ,, ");

  it("parses the localized keyword list", () => {
    expect(keywords).toEqual(["attached", "enclosed"]);
  });

  it("finds keywords in the user's own text only", () => {
    expect(mentionsAttachment("See the attached file.", keywords)).toBe(true);
    expect(mentionsAttachment("unattached", keywords)).toBe(false);
    expect(ownText("plain", "Hi\n> see attached\n-- \nattached sig")).toBe("Hi");
    expect(
      ownText("html", '<p>Hi</p><blockquote>attached</blockquote><div class="signature">x</div>'),
    ).toBe("Hi");
    expect(needsReminder("plain", "Hi\n> attached", "", 0, keywords)).toBe(false);
    expect(needsReminder("markdown", "The report is **attached**", "", 0, keywords)).toBe(true);
    expect(needsReminder("plain", "attached", "", 1, keywords)).toBe(false);
    expect(needsReminder("plain", "", "Report attached", 0, keywords)).toBe(true);
  });
});

describe("address entries", () => {
  it("finds and completes the entry being typed", () => {
    expect(currentEntry('"Doe, J." <j@x.org>, gr')).toEqual({ start: 20, text: "gr" });
    expect(
      completeEntry("bob@x.org; gr", { name: "Grace Hopper", email: "grace@navy.example" }),
    ).toBe("bob@x.org; Grace Hopper <grace@navy.example>, ");
    expect(completeEntry("gr", { name: null, email: "grace@navy.example" })).toBe(
      "grace@navy.example, ",
    );
  });
});

describe("composer mock", () => {
  it("renders the Markdown subset", () => {
    const html = renderMarkdown(
      "# Title\n\nSome **bold** and *it* `x<y`\n\n- a\n- b\n\n> quoted\n\n```js\nlet a = 1 < 2;\n```",
    );
    expect(html).toContain("<h1>Title</h1>");
    expect(html).toContain("<strong>bold</strong> and <em>it</em> <code>x&lt;y</code>");
    expect(html).toContain("<ul><li>a</li><li>b</li></ul>");
    expect(html).toContain("<blockquote><p>quoted</p></blockquote>");
    expect(html).toContain('<pre><code class="language-js">let a = 1 &lt; 2;</code></pre>');
  });

  it("converts drafts and flags lossy switches", () => {
    const converted = (d: Draft, mode: Draft["mode"]) =>
      convertDraft(d, mode) as Extract<Response, { type: "DraftConverted" }>;
    const toHtml = converted(draft({ text: "a <b>\nc" }), "html");
    expect(toHtml.lossy).toBe(false);
    expect(toHtml.draft.html).toBe("<p>a &lt;b&gt;<br>c</p>");
    const toMd = converted(
      draft({ mode: "html", html: "<p><strong>x</strong> <u>y</u></p>" }),
      "markdown",
    );
    expect(toMd.lossy).toBe(true);
    expect(toMd.draft.markdown).toBe("**x** y");
    const mdPlain = converted(draft({ mode: "markdown", markdown: "plain words" }), "plain");
    expect(mdPlain.lossy).toBe(false);
    expect(mdPlain.draft.text).toBe("plain words");
    expect(converted(draft({ mode: "markdown", markdown: "**b**" }), "plain").lossy).toBe(true);
    const htmlPlain = converted(draft({ mode: "html", html: "<p>a<br>b</p>" }), "plain");
    expect(htmlPlain).toMatchObject({ lossy: false, draft: { text: "a\nb", html: null } });
  });

  it("suggests collected addresses by address or name", () => {
    const transport = new MockTransport({ data: createMockData(30) });
    expect(suggest(transport, "gr", 5).map((a) => a.email)).toEqual(["grace@navy.example"]);
    expect(suggest(transport, "hopper", 5).map((a) => a.email)).toEqual(["grace@navy.example"]);
    expect(suggest(transport, "ada", 5)).toEqual([]);
    expect(suggest(transport, " ", 5)).toEqual([]);
    transport.data.features.composer.addresses.push({ name: "Grete", email: "grete@x.example" });
    expect(suggest(transport, "gr", 1)).toHaveLength(1);
  });

  it("stores uploads and rich drafts", async () => {
    const transport = new MockTransport({ data: createMockData(5) });
    const client = new TransportClient(transport);
    const { attachment } = await client.request(
      {
        type: "UploadAttachment",
        filename: "a.txt",
        mime: "text/plain",
        data: btoa("hello"),
        content_id: null,
      },
      "AttachmentUploaded",
    );
    expect(attachment).toMatchObject({ filename: "a.txt", size: 5, content_id: null });
    await expect(
      client.request(
        { type: "UploadAttachment", filename: "x", mime: "x/y", data: "!!", content_id: null },
        "AttachmentUploaded",
      ),
    ).rejects.toMatchObject({ code: "invalid_request" });
    const big = "A".repeat(Math.ceil((MAX_ATTACHMENT_BYTES * 4) / 3) + 8);
    await expect(
      client.request(
        { type: "UploadAttachment", filename: "big", mime: "x/y", data: big, content_id: null },
        "AttachmentUploaded",
      ),
    ).rejects.toMatchObject({ code: "too_large" });

    const saved = saveRichDraft(
      transport,
      draft({ mode: "html", html: "<p>x</p>", attachments: [attachment] }),
    );
    expect(saved).toMatchObject({ type: "DraftSaved" });
    expect(
      saveRichDraft(transport, draft({ attachments: [{ ...attachment, blob: "nope" }] })),
    ).toMatchObject({
      type: "Error",
      code: "invalid_request",
    });
    expect(saveRichDraft(transport, draft({ id: "draft-404" }))).toMatchObject({
      code: "not_found",
    });
    expect(saveRichDraft(transport, draft({ identity_id: "who" }))).toMatchObject({
      code: "invalid_request",
    });
  });

  it("answers NewDraft with quote, recipients and signature", async () => {
    const transport = new MockTransport({ data: createMockData(5) });
    const client = new TransportClient(transport);
    const message = firstMessage(transport);
    const { draft: reply } = await client.request(
      {
        type: "NewDraft",
        identity_id: "ada@work.example",
        account_id: null,
        mode: "markdown",
        context: { mode: "reply", message_id: message.id },
      },
      "Draft",
    );
    expect(reply.subject).toMatch(/^Re: /);
    expect(reply.to.map((a) => a.email)).toEqual([message.from?.email]);
    expect(reply.markdown).toContain("wrote:\n> ");
    expect(reply.markdown).toMatch(/-- \nAda Lovelace\nAnalyst$/);
    const { draft: fresh } = await client.request(
      {
        type: "NewDraft",
        identity_id: null,
        account_id: "acc-personal",
        mode: "html",
        context: null,
      },
      "Draft",
    );
    expect(fresh).toMatchObject({ identity_id: "ada@example.org", mode: "html", text: "" });
    expect(fresh.html).toContain('<div class="signature"><p>-- </p><p>Ada</p></div>');
    await expect(
      client.request(
        {
          type: "NewDraft",
          identity_id: null,
          account_id: null,
          mode: null,
          context: { mode: "forward", message_id: "missing" },
        },
        "Draft",
      ),
    ).rejects.toMatchObject({ code: "not_found" });
  });
});

describe("composer flows", () => {
  it("starts a new message from NewDraft in the identity's mode", async () => {
    const ctx = context();
    firstIdentity(ctx.transport).composer_mode = "markdown";
    await startRichCompose(ctx);
    const ui = ctx.store.getState();
    // M5c: a window of its own (no draft id yet: `w1`).
    expect(ui.windows).toEqual(["composer:w1"]);
    expect(ui.composers["composer:w1"]).toMatchObject({ identityId: "ada@example.org" });
    // The focused composer is mirrored for the single-composer readers.
    expect(ui.composer).toMatchObject({ identityId: "ada@example.org", text: "" });
    expect(activeRich(ctx).getState()).toMatchObject({
      mode: "markdown",
      markdown: "\n\n-- \nAda",
    });
    // A second call opens a second composer; the first stays open.
    await startRichCompose(ctx);
    expect(ctx.transport.requests.filter((r) => r.type === "NewDraft")).toHaveLength(2);
    expect(ctx.store.getState().windows).toEqual(["composer:w1", "composer:w2"]);
  });

  it("falls back to the plain-text flows when the core has no composer yet", async () => {
    const ctx = context();
    const send = ctx.transport.send.bind(ctx.transport);
    ctx.transport.send = async (request) =>
      request.type === "NewDraft"
        ? { type: "Error", code: "unsupported", message: "not yet" }
        : send(request);
    await startRichCompose(ctx);
    expect(ctx.store.getState().composer?.text).toBe("\n\n-- \nAda");
    expect(activeRich(ctx).getState().mode).toBe("plain");
    // An older flow clearing the mirror closes the focused composer.
    ctx.store.getState().setComposer(null);
    expect(ctx.store.getState().windows).toEqual([]);
    const id = firstMessage(ctx.transport).id;
    await startRichReply(ctx, id, "reply");
    expect(ctx.store.getState().composer?.context).toEqual({ mode: "reply", message_id: id });
  });

  it("opens a plain-text composer when the core cannot choose an identity", async () => {
    const ctx = context();
    const send = ctx.transport.send.bind(ctx.transport);
    ctx.transport.send = async (request) =>
      request.type === "NewDraft"
        ? { type: "Error", code: "invalid_request", message: "there is no identity to send from" }
        : send(request);
    await startRichCompose(ctx);
    expect(ctx.store.getState().composer).not.toBeNull();
    expect(ctx.store.getState().windows).toEqual(["composer:w1"]);
    expect(activeRich(ctx).getState().mode).toBe("plain");
  });

  it("replies in HTML and sends the rich draft", async () => {
    const ctx = context();
    const id = firstMessage(ctx.transport).id;
    firstIdentity(ctx.transport).composer_mode = "html";
    await startRichReply(ctx, id, "reply-all");
    const window = activeComposer(ctx.store.getState().windows) ?? "";
    const rich = richStoreFor(ctx.store, window);
    expect(rich.getState().mode).toBe("html");
    expect(rich.getState().html).toContain("<blockquote");
    expect(await sendRich(ctx, null)).toBe(true);
    const saved = ctx.transport.requests.find((r) => r.type === "SaveDraft");
    expect(saved).toMatchObject({ draft: { mode: "html", context: { mode: "reply-all" } } });
    expect(ctx.store.getState().composer).toBeNull();
    expect(ctx.store.getState().windows).toEqual([]);
    expect(ctx.store.getState().composers).toEqual({});
    // Its rich state went with the window.
    expect(instanceRichStore(ctx.store, window)).toBeUndefined();
  });

  it("switches modes, waiting for confirmation when formatting is lost", async () => {
    const ctx = context();
    ctx.store
      .getState()
      .setComposer({ ...emptyComposer(), identityId: "ada@example.org", text: "Hi *there*" });
    const rich = richStoreFor(ctx.store);
    await switchMode(ctx, "markdown");
    expect(rich.getState()).toMatchObject({ mode: "markdown", markdown: "Hi *there*" });
    expect(ctx.store.getState().composer?.text).toBe("");
    await switchMode(ctx, "html");
    expect(rich.getState().html).toBe("<p>Hi <em>there</em></p>");
    rich.setState({ html: "<p><u>under</u></p>" });
    await switchMode(ctx, "markdown");
    expect(rich.getState()).toMatchObject({ mode: "html", pendingSwitch: { mode: "markdown" } });
    cancelSwitch(ctx);
    expect(rich.getState()).toMatchObject({ mode: "html", pendingSwitch: null });
    await switchMode(ctx, "plain");
    confirmSwitch(ctx);
    expect(rich.getState()).toMatchObject({ mode: "plain", html: "", pendingSwitch: null });
    expect(ctx.store.getState().composer?.text).toBe("under");
    // Nothing happens without a composer or for the current mode.
    await switchMode(ctx, "plain");
    expect(ctx.transport.requests.filter((r) => r.type === "ConvertDraft")).toHaveLength(4);
  });

  it("reports conversion failures", async () => {
    const ctx = context();
    ctx.store.getState().setComposer({ ...emptyComposer(), identityId: "ada@example.org" });
    ctx.transport.send = () =>
      Promise.resolve({ type: "Error", code: "unsupported", message: "no" });
    await switchMode(ctx, "html");
    expect(ctx.store.getState().composer?.error).toEqual({ id: "error-unsupported" });
    expect(richStoreFor(ctx.store).getState()).toMatchObject({ mode: "plain", switching: false });
  });

  it("uploads attachments and inline images", async () => {
    const ctx = context();
    ctx.store.getState().setComposer({ ...emptyComposer(), identityId: "ada@example.org" });
    const file = new File(["hello"], "notes.txt", { type: "text/plain" });
    const image = new File([new Uint8Array([137, 80, 78, 71])], "logo.png", { type: "image/png" });
    const [plain] = await uploadFiles(ctx, [file]);
    const [inline] = await uploadFiles(ctx, [image], true);
    const state = richStoreFor(ctx.store).getState();
    expect(plain).toMatchObject({ filename: "notes.txt", size: 5, content_id: null });
    expect(inline?.content_id).toMatch(/@nail$/);
    expect(state.attachments).toHaveLength(2);
    expect(state.inlinePreviews[inline?.content_id ?? ""]).toMatch(/^data:image\/png;base64,/);
    expect(state.uploads).toBe(0);
    const request = ctx.transport.requests.find((r) => r.type === "UploadAttachment");
    expect(request).toMatchObject({ data: btoa("hello"), mime: "text/plain" });
  });

  it("asks before sending when an attachment is mentioned but missing", async () => {
    const ctx = context();
    ctx.store.getState().setComposer({
      ...emptyComposer(),
      identityId: "ada@example.org",
      to: "grace@navy.example",
      text: "The report is attached.",
    });
    expect(await sendRich(ctx, null)).toBe(false);
    expect(richStoreFor(ctx.store).getState().reminder).toBe("confirm");
    expect(ctx.transport.requests.some((r) => r.type === "SendDraft")).toBe(false);
    expect(await sendRich(ctx, null, { force: true })).toBe(true);
  });

  it("refuses to send while uploads run and validates recipients", async () => {
    const ctx = context();
    ctx.store
      .getState()
      .setComposer({ ...emptyComposer(), identityId: "ada@example.org", to: "x" });
    expect(await sendRich(ctx, null)).toBe(false);
    expect(ctx.store.getState().composer?.error?.id).toBe("composer-error-invalid-address");
    ctx.store.getState().updateComposer({ to: "" });
    expect(await sendRich(ctx, null)).toBe(false);
    expect(ctx.store.getState().composer?.error?.id).toBe("composer-error-no-recipients");
    ctx.store.getState().updateComposer({ to: "grace@navy.example" });
    richStoreFor(ctx.store).setState({ uploads: 1 });
    expect(await sendRich(ctx, null)).toBe(false);
    expect(ctx.store.getState().composer?.error?.id).toBe("composer-error-uploading");
    richStoreFor(ctx.store).setState({ uploads: 0 });
    expect(await sendRich(ctx, "not a date")).toBe(false);
    expect(ctx.store.getState().composer?.error?.id).toBe("composer-error-send-at");
  });

  it("saves quietly (autosave) and once at a time", async () => {
    const ctx = context();
    ctx.store
      .getState()
      .setComposer({ ...emptyComposer(), identityId: "ada@example.org", subject: "A" });
    const rich = richStoreFor(ctx.store);
    rich.setState({ mode: "markdown", markdown: "**x**" });
    const [first, second] = await Promise.all([
      saveRich(ctx, { quiet: true }),
      saveRich(ctx, { quiet: true }),
    ]);
    expect(first).toBe(second);
    expect(ctx.transport.data.drafts.size).toBe(1);
    expect(ctx.transport.data.drafts.get(first ?? "")?.markdown).toBe("**x**");
    expect(rich.getState().autosave).toBe("saved");
    expect(ctx.store.getState().composer?.busy).toBeNull();
    ctx.store.getState().updateComposer({ identityId: null });
    expect(await saveRich(ctx, { quiet: true })).toBeNull();
    expect(ctx.store.getState().composer?.error).toBeNull();
    ctx.store.getState().updateComposer({ identityId: "nobody" });
    expect(await saveRich(ctx, { quiet: true })).toBeNull();
    expect(rich.getState().autosave).toBe("failed");
  });

  it("reopens saved drafts in their mode", async () => {
    const ctx = context();
    ctx.transport.data.drafts.set(
      "draft-7",
      draft({ id: "draft-7", mode: "markdown", markdown: "# D" }),
    );
    await openRichDraft(ctx, "draft-7");
    expect(ctx.store.getState().composer).toMatchObject({
      draftId: "draft-7",
      identityPicked: true,
    });
    expect(ctx.store.getState().windows).toEqual(["composer:draft-7"]);
    const rich = activeRich(ctx).getState();
    expect(rich).toMatchObject({ mode: "markdown", markdown: "# D", autosave: "saved" });
    expect(rich.savedKey).not.toBeNull();
    // Opening it again focuses the open window.
    await openRichDraft(ctx, "draft-7");
    expect(ctx.store.getState().windows).toEqual(["composer:draft-7"]);
    expect(ctx.transport.requests.filter((r) => r.type === "GetDraft")).toHaveLength(1);
  });

  it("swaps the signature when the identity changes", () => {
    const ctx = context();
    const ids = ctx.transport.data.accountsConfig.accounts.flatMap((a) => a.identities);
    const personal = must(ids.find((i) => i.id === "ada@example.org"));
    const work = must(ids.find((i) => i.id === "ada@work.example"));
    ctx.store.getState().setComposer({ ...emptyComposer(), text: "Hi\n\n-- \nAda" });
    swapSignature(ctx, personal, work);
    expect(ctx.store.getState().composer?.text).toBe("Hi\n\n-- \nAda Lovelace\nAnalyst");
    const rich = richStoreFor(ctx.store);
    rich.setState({ mode: "markdown", markdown: "x\n\n-- \nAda Lovelace\nAnalyst" });
    swapSignature(ctx, work, { ...personal, signature: null });
    expect(rich.getState().markdown).toBe("x\n\n");
    rich.setState({ signature: "none", markdown: "-- \nAda" });
    swapSignature(ctx, personal, work);
    expect(rich.getState().markdown).toBe("-- \nAda");
  });

  it("resets the rich state for a new session unless primed", () => {
    const ctx = context();
    const rich = richStoreFor(ctx.store);
    rich.setState({ mode: "html", html: "<p>x</p>" });
    ctx.store.getState().setComposer({ ...emptyComposer() });
    expect(rich.getState().mode).toBe("plain");
    ctx.store.getState().setComposer(null);
    primeRich(ctx.store, { mode: "markdown", markdown: "m" });
    ctx.store.getState().setComposer({ ...emptyComposer() });
    expect(rich.getState()).toMatchObject({ mode: "markdown", markdown: "m", primed: false });
    ctx.store.getState().setComposer(null);
    expect(rich.getState().mode).toBe("plain");
  });
});
