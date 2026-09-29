import { QueryClient } from "@tanstack/react-query";
import { TransportClient } from "../api/client";
import type { MessageSummary } from "../api/generated";
import { MockTransport } from "../api/mock";
import { patternMatches } from "../api/mockAccounts";
import { createMockData } from "../api/mockData";
import { displayName, formatAddress, formatAddressList, parseAddressList } from "./addresses";
import {
  cancelSend,
  chooseIdentity,
  discardComposer,
  draftOf,
  emptyComposer,
  saveComposer,
  sendComposer,
  startCompose,
  startReply,
  type ComposeContext,
} from "./compose";
import { deleteMessages, findSummary, moveMessages, nextSelection, setFlags } from "./mailOps";
import { queryKeys } from "./queries";
import { createUiStore } from "./uiStore";

function context(state = {}): ComposeContext & { transport: MockTransport } {
  const transport = new MockTransport({ data: createMockData(20) });
  return {
    transport,
    client: new TransportClient(transport),
    store: createUiStore({ activeAccountId: "acc-personal", ...state }),
    queryClient: new QueryClient({ defaultOptions: { queries: { retry: false } } }),
    t: (id, args) => (args ? `${id} ${JSON.stringify(args)}` : id),
    locale: "en",
  };
}

/** Puts a folder's first page into the cache like the message list does. */
async function loadFolder(ctx: ComposeContext, folderId: string) {
  const page = await ctx.client.request(
    { type: "ListMessages", folder_id: folderId, offset: 0, limit: 100 },
    "Messages",
  );
  ctx.queryClient.setQueryData(queryKeys.messages(folderId, 100), {
    pages: [page],
    pageParams: [0],
  });
  ctx.store.getState().setMessageOrder(page.messages.map((m) => m.id));
  return page.messages;
}

describe("addresses", () => {
  it("parses recipient fields", () => {
    expect(
      parseAddressList('Ada <ada@example.org>, "Doe, J." <j@x.org>; bob@y.org, nope, '),
    ).toEqual({
      addresses: [
        { name: "Ada", email: "ada@example.org" },
        { name: "Doe, J.", email: "j@x.org" },
        { name: null, email: "bob@y.org" },
      ],
      invalid: ["nope"],
    });
    expect(parseAddressList("Ann <not-an-address>").invalid).toEqual(["Ann <not-an-address>"]);
  });

  it("formats addresses back into editable text", () => {
    expect(formatAddress({ name: "Doe, J.", email: "j@x.org" })).toBe('"Doe, J." <j@x.org>');
    expect(
      formatAddressList([
        { name: "Ada", email: "a@x.org" },
        { name: null, email: "b@x.org" },
      ]),
    ).toBe("Ada <a@x.org>, b@x.org");
    expect(displayName({ name: null, email: "b@x.org" })).toBe("b@x.org");
    expect(displayName(null)).toBe("");
  });

  it("matches identity rule patterns", () => {
    expect(patternMatches("*@client.com", "Bob@Client.com")).toBe(true);
    expect(patternMatches("*@client.com", "bob@eu.client.com")).toBe(false);
    expect(patternMatches("*@*.client.com", "bob@eu.client.com")).toBe(true);
    expect(patternMatches("bob@client.com", "bob@client.com")).toBe(true);
  });
});

describe("message writes", () => {
  it("picks the next message after removals", () => {
    expect(nextSelection(["a", "b", "c"], "b", ["b"])).toBe("c");
    expect(nextSelection(["a", "b", "c"], "c", ["c"])).toBe("b");
    expect(nextSelection(["a", "b"], "a", ["a", "b"])).toBeNull();
    expect(nextSelection(["a", "b"], "a", ["b"])).toBe("a");
  });

  it("sets flags optimistically and queues the change", async () => {
    const ctx = context();
    const messages = await loadFolder(ctx, "acc-personal/Inbox");
    const unread = messages.find((m) => !m.flags.seen) as MessageSummary;
    const pending = setFlags(ctx, [unread.id], { seen: true });
    expect(findSummary(ctx.queryClient, unread.id)?.flags.seen).toBe(true);
    await pending;
    expect(ctx.transport.requests.at(-1)).toMatchObject({
      type: "UpdateFlags",
      change: { seen: true, flagged: null },
    });
    expect(ctx.store.getState().pendingOps).toHaveLength(1);
  });

  it("removes moved and deleted messages from the list and selects the next one", async () => {
    const ctx = context();
    const messages = await loadFolder(ctx, "acc-personal/Inbox");
    const [first, second, third] = messages as [MessageSummary, MessageSummary, MessageSummary];
    ctx.store.getState().selectMessage(first.id);
    await moveMessages(ctx, [first.id], { role: "archive" });
    expect(findSummary(ctx.queryClient, first.id)).toBeUndefined();
    expect(ctx.store.getState().selectedMessageId).toBe(second.id);
    await deleteMessages(ctx, [second.id], false);
    expect(ctx.store.getState().selectedMessageId).toBe(third.id);
    expect(ctx.transport.data.messages.find((m) => m.id === second.id)?.folder_id).toBe(
      "acc-personal/Trash",
    );
  });

  it("reports a rejected write and shows the core's state again", async () => {
    const ctx = context();
    await loadFolder(ctx, "acc-personal/Inbox");
    await expect(setFlags(ctx, ["gone"], { flagged: true })).rejects.toMatchObject({
      code: "not_found",
    });
    expect(ctx.store.getState().opError).toBe("not_found");
    await expect(setFlags(ctx, ["gone"], { seen: true }, { quiet: true })).rejects.toBeDefined();
  });
});

describe("composer flows", () => {
  it("starts a new message from the account's default identity with its signature", async () => {
    const ctx = context();
    await startCompose(ctx);
    const { composer, windows } = ctx.store.getState();
    expect(windows).toEqual(["composer"]);
    expect(composer).toMatchObject({ identityId: "ada@example.org", to: "" });
    expect(composer?.text).toBe("\n\n-- \nAda");
    // A second start resumes the unfinished message.
    ctx.store.getState().updateComposer({ subject: "Draft in progress" });
    ctx.store.getState().closeWindow("composer");
    await startCompose(ctx);
    expect(ctx.store.getState().composer?.subject).toBe("Draft in progress");
  });

  it("uses a virtual account's default identity", async () => {
    const ctx = context({ activeAccountId: "team" });
    ctx.transport.data.accountsConfig.virtual_accounts.push({
      id: "team",
      name: "Team",
      members: ["acc-personal", "acc-work"],
      merge_roles: true,
      folder_mappings: [],
      default_identity: "ada@work.example",
      identity_rules: [],
    });
    await startCompose(ctx);
    expect(ctx.store.getState().composer?.identityId).toBe("ada@work.example");
  });

  it("replies to the sender, quoting the original, from the addressed identity", async () => {
    const ctx = context();
    const id = "acc-work/Inbox#1";
    await startReply(ctx, id, "reply");
    const c = ctx.store.getState().composer;
    expect(c).toMatchObject({
      identityId: "ada@work.example",
      to: "Edsger Dijkstra <ewd@tue.example>",
      context: { mode: "reply", message_id: id },
    });
    expect(c?.subject).toMatch(/^Re: /);
    expect(c?.text).toContain("composer-quote-header");
    expect(c?.text).toContain("> Hello Ada,");
    expect(c?.text).toContain(">> Earlier text");
  });

  it("replies to all without our own addresses, and forwards without a recipient", async () => {
    const ctx = context();
    const id = "acc-personal/Inbox#1"; // cc: Alan Turing
    await startReply(ctx, id, "reply-all");
    const c = ctx.store.getState().composer;
    expect(c?.cc).toBe("Alan Turing <alan@bletchley.example>");
    expect(c?.cc).not.toContain("ada@example.org");
    expect(c?.showCc).toBe(true);

    ctx.store.getState().setComposer(null);
    await startReply(ctx, id, "forward");
    const f = ctx.store.getState().composer;
    expect(f?.to).toBe("");
    expect(f?.subject).toBe("Fwd: Compiler notes for Monday");
    expect(f?.context).toEqual({ mode: "forward", message_id: id });
    expect(f?.text).toContain("composer-forward-header");
  });

  it("asks the core which identity to send from", async () => {
    const ctx = context();
    // A reply goes out from the identity the original was sent to.
    expect((await chooseIdentity(ctx, "acc-personal", [], "acc-work/Inbox#1"))?.id).toBe(
      "ada@work.example",
    );
    expect((await chooseIdentity(ctx, "acc-personal", [], null))?.id).toBe("ada@example.org");
    // A virtual account's rules: the first rule matching any recipient.
    ctx.transport.data.accountsConfig.virtual_accounts.push({
      id: "team",
      name: "Team",
      members: ["acc-personal", "acc-work"],
      merge_roles: true,
      folder_mappings: [],
      default_identity: "ada@example.org",
      identity_rules: [
        { pattern: "*@a.example", identity: "ada@work.example" },
        { pattern: "*@b.example", identity: "ada@example.org" },
      ],
    });
    expect((await chooseIdentity(ctx, "team", ["x@b.example", "y@a.example"], null))?.id).toBe(
      "ada@work.example",
    );
    expect((await chooseIdentity(ctx, "team", ["x@c.example"], null))?.id).toBe("ada@example.org");
    const asked = ctx.transport.requests.filter((r) => r.type === "ChooseIdentity");
    expect(asked).toHaveLength(4);
  });

  it("validates before sending", async () => {
    const ctx = context();
    ctx.store.getState().setComposer({ ...emptyComposer(), identityId: "ada@example.org" });
    expect(await sendComposer(ctx, null)).toBe(false);
    expect(ctx.store.getState().composer?.error?.id).toBe("composer-error-no-recipients");
    ctx.store.getState().updateComposer({ to: "grace@navy.example, oops" });
    expect(await sendComposer(ctx, null)).toBe(false);
    expect(ctx.store.getState().composer?.error).toEqual({
      id: "composer-error-invalid-address",
      args: { addresses: "oops" },
    });
    ctx.store.getState().updateComposer({ to: "grace@navy.example" });
    expect(await sendComposer(ctx, "not a date")).toBe(false);
    expect(ctx.store.getState().composer?.error?.id).toBe("composer-error-send-at");
    ctx.store.getState().updateComposer({ identityId: null });
    expect(await saveComposer(ctx)).toBeNull();
    expect(ctx.store.getState().composer?.error?.id).toBe("composer-error-no-identity");
  });

  it("saves, sends with undo-send, and undoes back into the composer", async () => {
    const ctx = context();
    ctx.store.getState().setComposer({
      ...emptyComposer(),
      identityId: "ada@example.org",
      to: "Grace <grace@navy.example>",
      subject: "Plans",
      text: "See you",
    });
    ctx.store.getState().openWindow("composer");
    expect(draftOf(ctx.store.getState().composer ?? emptyComposer()).draft.to).toEqual([
      { name: "Grace", email: "grace@navy.example" },
    ]);
    expect(await sendComposer(ctx, null)).toBe(true);
    const { lastSend, composer, windows } = ctx.store.getState();
    expect(lastSend?.state).toBe("scheduled");
    expect(composer).toBeNull();
    expect(windows).toEqual([]);

    await cancelSend(ctx, lastSend?.id ?? "");
    expect(ctx.store.getState().lastSend).toBeNull();
    expect(ctx.store.getState().composer).toMatchObject({
      subject: "Plans",
      to: "Grace <grace@navy.example>",
      text: "See you",
    });
    expect(ctx.store.getState().windows).toEqual(["composer"]);

    // Discarding deletes the saved draft.
    const draftId = ctx.store.getState().composer?.draftId ?? "";
    await discardComposer(ctx);
    expect(ctx.transport.data.drafts.has(draftId)).toBe(false);
    expect(ctx.store.getState().composer).toBeNull();
  });

  it("sends later at the chosen time", async () => {
    const ctx = context();
    ctx.store.getState().setComposer({
      ...emptyComposer(),
      identityId: "ada@example.org",
      to: "grace@navy.example",
    });
    const at = new Date(Date.now() + 86_400_000).toISOString();
    expect(await sendComposer(ctx, at)).toBe(true);
    expect(ctx.transport.requests.at(-1)).toMatchObject({ type: "SendDraft", send_at: at });
    expect(ctx.store.getState().lastSend?.send_at).toBe(at);
  });
});
