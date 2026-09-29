/**
 * The mock core behaves like a real one for the M1a requests, so the widget
 * tests exercise realistic flows.
 */
import { TransportClient } from "./client";
import type { AccountConfig, Draft, Event as NailEvent } from "./generated";
import { MockTransport } from "./mock";
import { createMockData, LIST_MESSAGE_ID } from "./mockData";

function setup(inboxSize = 20) {
  const transport = new MockTransport({ data: createMockData(inboxSize) });
  const events: NailEvent[] = [];
  transport.onEvent((e) => events.push(e));
  return { transport, client: new TransportClient(transport), events };
}

const draft = (patch: Partial<Draft> = {}): Draft => ({
  id: null,
  identity_id: "ada@example.org",
  to: [{ name: "Grace", email: "grace@navy.example" }],
  cc: [],
  bcc: [],
  subject: "Hello",
  mode: "plain",
  text: "Hi Grace",
  html: null,
  markdown: null,
  attachments: [],
  in_reply_to: null,
  ...patch,
});

afterEach(() => vi.useRealTimers());

describe("virtual accounts in the mock core", () => {
  it("merges role folders of all accounts and keeps the rest in per-account subtrees", async () => {
    const { client } = setup();
    const { folders } = await client.request({ type: "ListFolders", account_id: "all" }, "Folders");
    const inbox = folders.find((f) => f.id === "all/inbox");
    expect(inbox).toMatchObject({ role: "inbox", account_id: "all", total: 20 + 12 });
    const subtree = folders.find((f) => f.id === "all/account-acc-personal");
    expect(subtree?.name).toBe("Personal");
    expect(folders.find((f) => f.name === "Receipts")?.parent_id).toBe(subtree?.id);
  });

  it("lists merged folders newest first and collapses duplicates by Message-ID", async () => {
    const { client, transport } = setup();
    const page = await client.request(
      { type: "ListMessages", folder_id: "all/archive", offset: 0, limit: 50 },
      "Messages",
    );
    const lists = page.messages.filter((m) => m.subject.startsWith("[rust-announce]"));
    expect(lists).toHaveLength(1);
    expect(lists[0]?.duplicates).toHaveLength(1);
    expect(transport.data.bodies.get(lists[0]?.id ?? "")?.headers.message_id).toBe(LIST_MESSAGE_ID);
    const dates = page.messages.map((m) => Date.parse(m.date));
    expect([...dates].sort((a, b) => b - a)).toEqual(dates);
  });

  it("applies hand-made folder mappings of user virtual accounts", async () => {
    const { client } = setup();
    const { config } = await client.request(
      {
        type: "AddVirtualAccount",
        config: {
          id: "",
          name: "Paper trail",
          members: ["acc-personal", "acc-work"],
          merge_roles: false,
          folder_mappings: [
            {
              name: "Receipts",
              sources: [
                { account_id: "acc-personal", path: "Inbox/Receipts" },
                { account_id: "acc-work", path: "Inbox/Receipts" },
              ],
            },
          ],
          default_identity: "ada@work.example",
          identity_rules: [{ pattern: "*@client.example", identity: "ada@work.example" }],
        },
      },
      "VirtualAccountSaved",
    );
    expect(config.id).toBe("paper-trail");
    const { accounts } = await client.request({ type: "ListAccounts" }, "Accounts");
    expect(accounts.at(-1)).toEqual({
      id: "paper-trail",
      name: "Paper trail",
      email: "",
      kind: "virtual",
    });
    const { folders } = await client.request(
      { type: "ListFolders", account_id: "paper-trail" },
      "Folders",
    );
    expect(folders.find((f) => f.name === "Receipts")?.total).toBe(6);
    expect(folders.some((f) => f.role === "inbox" && f.parent_id === null)).toBe(false);
    const { identities } = await client.request(
      { type: "ListIdentities", account_id: "paper-trail" },
      "Identities",
    );
    expect(identities.map((i) => i.id)).toEqual([
      "ada@example.org",
      "ada+lists@example.org",
      "ada@work.example",
    ]);
  });

  it("validates virtual accounts", async () => {
    const { client } = setup();
    const response = await client.call({
      type: "AddVirtualAccount",
      config: {
        id: "",
        name: "Nobody",
        members: [],
        merge_roles: true,
        folder_mappings: [],
        default_identity: null,
        identity_rules: [],
      },
    });
    expect(response).toMatchObject({ type: "Error", code: "invalid_request" });
    expect(await client.call({ type: "RemoveVirtualAccount", account_id: "all" })).toMatchObject({
      code: "invalid_request",
    });
  });
});

describe("writes in the mock core", () => {
  it("queues flag changes per account and reports their replay", async () => {
    const { client, transport, events } = setup();
    const ids = ["acc-personal/Inbox#1", "acc-work/Inbox#1"];
    const { op_ids } = await client.request(
      {
        type: "UpdateFlags",
        message_ids: ids,
        change: {
          seen: true,
          flagged: true,
          answered: null,
          draft: null,
          add_tags: ["x"],
          remove_tags: [],
        },
      },
      "Queued",
    );
    expect(op_ids).toHaveLength(2);
    const m = transport.data.messages.find((x) => x.id === ids[0]);
    expect(m?.flags).toMatchObject({ seen: true, flagged: true });
    expect(m?.tags).toContain("x");
    expect(events[0]).toMatchObject({ type: "MailChanged" });
    await vi.waitFor(() =>
      expect(events.filter((e) => e.type === "OpStatus" && e.status.state === "done")).toHaveLength(
        2,
      ),
    );
  });

  it("reports failed replays when told to", async () => {
    const { client, transport, events } = setup();
    transport.data.behaviour.opError = "network";
    await client.request(
      { type: "MoveMessages", message_ids: ["acc-personal/Inbox#2"], target: { role: "archive" } },
      "Queued",
    );
    await vi.waitFor(() =>
      expect(
        events.find((e) => e.type === "OpStatus" && e.status.state === "failed"),
      ).toMatchObject({
        status: { error: "network" },
      }),
    );
  });

  it("moves by role and by (virtual) folder, resolving to the source account", async () => {
    const { client, transport } = setup();
    await client.request(
      {
        type: "MoveMessages",
        message_ids: ["acc-work/Inbox#1"],
        target: { folder_id: "all/archive" },
      },
      "Queued",
    );
    expect(transport.data.messages.find((m) => m.id === "acc-work/Inbox#1")?.folder_id).toBe(
      "acc-work/Archive",
    );
    expect(
      await client.call({
        type: "MoveMessages",
        message_ids: ["nope"],
        target: { role: "archive" },
      }),
    ).toMatchObject({ code: "not_found" });
  });

  it("deletes to Trash, and for good from Trash", async () => {
    const { client, transport } = setup();
    const id = "acc-personal/Inbox#3";
    await client.request({ type: "DeleteMessages", message_ids: [id], permanent: false }, "Queued");
    expect(transport.data.messages.find((m) => m.id === id)?.folder_id).toBe("acc-personal/Trash");
    await client.request({ type: "DeleteMessages", message_ids: [id], permanent: false }, "Queued");
    expect(transport.data.messages.some((m) => m.id === id)).toBe(false);
  });
});

describe("threads and search in the mock core", () => {
  it("groups a folder into threads and returns a thread oldest first", async () => {
    const { client } = setup(24);
    const page = await client.request(
      { type: "ListThreads", folder_id: "acc-personal/Inbox", offset: 0, limit: 100 },
      "Threads",
    );
    const multi = page.threads.find((t) => t.message_count > 1);
    expect(multi).toBeDefined();
    const { messages, thread } = await client.request(
      { type: "GetThread", thread_id: multi?.id ?? "" },
      "Thread",
    );
    expect(thread.message_count).toBe(messages.length);
    expect(messages.at(-1)?.id).toBe(thread.latest_message_id);
    expect(Date.parse(messages[0]?.date ?? "")).toBeLessThan(
      Date.parse(messages.at(-1)?.date ?? ""),
    );
    expect(await client.call({ type: "GetThread", thread_id: "nope" })).toMatchObject({
      code: "not_found",
    });
  });

  it("searches within a scope and reports invalid queries", async () => {
    const { client } = setup();
    const results = await client.request(
      {
        type: "Search",
        query: {
          text: "is:flagged",
          account_id: "acc-work",
          folder_id: null,
          offset: 0,
          limit: 10,
          sort: "date",
        },
      },
      "SearchResults",
    );
    expect(results.total).toBeGreaterThan(0);
    expect(
      results.hits.every((h) => h.message.account_id === "acc-work" && h.message.flags.flagged),
    ).toBe(true);
    const invalid = await client.call({
      type: "Search",
      query: {
        text: "from:",
        account_id: null,
        folder_id: null,
        offset: 0,
        limit: 10,
        sort: "date",
      },
    });
    expect(invalid).toMatchObject({ type: "InvalidQuery", error: { kind: "unexpected-end" } });
    expect(await client.call({ type: "CheckQuery", text: "is:unread" })).toEqual({
      type: "QueryChecked",
      error: null,
    });
  });
});

describe("accounts in the mock core", () => {
  it("discovers settings, guesses, or finds nothing", async () => {
    const { client } = setup();
    const gmail = await client.request(
      { type: "DiscoverAccount", email: "ada@gmail.com" },
      "Discovered",
    );
    expect(gmail.candidates[0]?.incoming.server.auth).toMatchObject({
      method: "oauth2",
      provider: "google",
      username: "ada@gmail.com",
    });
    const guessed = await client.request(
      { type: "DiscoverAccount", email: "ada@example.net" },
      "Discovered",
    );
    expect(guessed.candidates[0]).toMatchObject({ source: "autoconfig" });
    expect(guessed.candidates[0]?.incoming.server.host).toBe("imap.example.net");
    const none = await client.request(
      { type: "DiscoverAccount", email: "a@b.invalid" },
      "Discovered",
    );
    expect(none.candidates).toEqual([]);
    expect(await client.call({ type: "DiscoverAccount", email: "nope" })).toMatchObject({
      code: "invalid_request",
    });
  });

  it("tests servers: success, bad password, unreachable host", async () => {
    const { client, transport } = setup();
    const config = structuredClone(transport.data.accountsConfig.accounts[0] as AccountConfig);
    const ok = await client.request(
      {
        type: "TestAccount",
        config,
        secrets: [{ endpoint: "incoming", value: { kind: "password", password: "secret" } }],
      },
      "AccountTested",
    );
    expect(ok.result.incoming).toMatchObject({ error: null });
    expect(ok.result.incoming.capabilities).toContain("IDLE");
    const wrong = await client.request(
      {
        type: "TestAccount",
        config,
        secrets: [{ endpoint: "incoming", value: { kind: "password", password: "wrong" } }],
      },
      "AccountTested",
    );
    expect(wrong.result.incoming.error).toBe("auth_failed");
    config.incoming.server.host = "unreachable.example";
    const down = await client.request(
      { type: "TestAccount", config, secrets: [] },
      "AccountTested",
    );
    expect(down.result.incoming.error).toBe("network");
    expect(down.result.outgoing?.error).toBeNull();
  });

  it("runs an OAuth sign-in and reports it with OAuthFinished", async () => {
    const { client, events } = setup();
    const started = await client.request(
      { type: "StartOAuth", provider: "google", login_hint: "ada@gmail.com" },
      "OAuthStarted",
    );
    await vi.waitFor(() =>
      expect(events).toContainEqual({
        type: "OAuthFinished",
        flow_id: started.flow_id,
        error: null,
        message: null,
      }),
    );
    expect(
      await client.call({ type: "StartOAuth", provider: "acme", login_hint: null }),
    ).toMatchObject({
      code: "config",
    });
  });

  it("adds, updates and removes accounts and announces the changes", async () => {
    const { client, transport, events } = setup();
    const config = structuredClone(transport.data.accountsConfig.accounts[1] as AccountConfig);
    const added = await client.request(
      {
        type: "AddAccount",
        config: { ...config, id: "", name: "Club", email: "ada@club.example", identities: [] },
        secrets: [],
      },
      "AccountSaved",
    );
    expect(added.config.id).toBe("club");
    expect(added.config.identities.map((i) => i.id)).toEqual(["ada@club.example"]);
    expect(events.map((e) => e.type)).toContain("AccountsChanged");
    await vi.waitFor(() =>
      expect(transport.data.statuses.find((s) => s.account_id === "club")?.state).toBe("online"),
    );
    expect(
      await client.call({ type: "AddAccount", config: { ...added.config }, secrets: [] }),
    ).toMatchObject({ code: "conflict" });

    await client.request(
      { type: "UpdateAccount", config: { ...added.config, name: "Chess club" }, secrets: [] },
      "AccountSaved",
    );
    const { accounts } = await client.request({ type: "ListAccounts" }, "Accounts");
    expect(accounts.find((a) => a.id === "club")?.name).toBe("Chess club");

    await client.request({ type: "RemoveAccount", account_id: "club" }, "Done");
    expect(transport.data.folders.some((f) => f.account_id === "club")).toBe(false);
    expect(await client.call({ type: "RemoveAccount", account_id: "club" })).toMatchObject({
      code: "not_found",
    });
  });
});

describe("drafts and the outbox in the mock core", () => {
  it("sends after the undo-send delay and files a Sent copy", async () => {
    vi.useFakeTimers();
    const { client, transport, events } = setup();
    const { draft_id } = await client.request({ type: "SaveDraft", draft: draft() }, "DraftSaved");
    const { entry } = await client.request(
      { type: "SendDraft", draft_id, delay_secs: null, send_at: null },
      "SendQueued",
    );
    expect(entry.state).toBe("scheduled");
    expect(Date.parse(entry.send_at) - Date.now()).toBe(10_000);
    expect((await client.request({ type: "ListOutbox" }, "Outbox")).entries).toHaveLength(1);
    await vi.advanceTimersByTimeAsync(10_000 + transport.data.behaviour.workMs);
    const states = events
      .filter((e) => e.type === "SendStatus")
      .map((e) => e.type === "SendStatus" && e.state);
    expect(states).toEqual(["sending", "sent"]);
    expect(transport.data.messages[0]).toMatchObject({
      folder_id: "acc-personal/Sent",
      subject: "Hello",
    });
    expect((await client.request({ type: "ListOutbox" }, "Outbox")).entries).toEqual([]);
    expect(await client.call({ type: "GetDraft", draft_id })).toMatchObject({ code: "not_found" });
  });

  it("cancels a scheduled message back into its draft, but not once sending started", async () => {
    vi.useFakeTimers();
    const { client } = setup();
    const { draft_id } = await client.request({ type: "SaveDraft", draft: draft() }, "DraftSaved");
    const first = await client.request(
      { type: "SendDraft", draft_id, delay_secs: 5, send_at: null },
      "SendQueued",
    );
    const cancelled = await client.request(
      { type: "CancelSend", outbox_id: first.entry.id },
      "SendCancelled",
    );
    expect(cancelled.draft_id).toBe(draft_id);
    expect((await client.request({ type: "GetDraft", draft_id }, "Draft")).draft.subject).toBe(
      "Hello",
    );

    const second = await client.request(
      { type: "SendDraft", draft_id, delay_secs: 0, send_at: null },
      "SendQueued",
    );
    await vi.advanceTimersByTimeAsync(1);
    expect(await client.call({ type: "CancelSend", outbox_id: second.entry.id })).toMatchObject({
      code: "conflict",
    });
  });

  it("schedules send-later messages and reports failures", async () => {
    vi.useFakeTimers();
    const { client, transport } = setup();
    transport.data.behaviour.sendError = "rejected";
    const { draft_id } = await client.request({ type: "SaveDraft", draft: draft() }, "DraftSaved");
    const at = new Date(Date.now() + 3_600_000).toISOString();
    const { entry } = await client.request(
      { type: "SendDraft", draft_id, delay_secs: null, send_at: at },
      "SendQueued",
    );
    expect(entry.send_at).toBe(at);
    await vi.advanceTimersByTimeAsync(3_600_000 + 100);
    const { entries } = await client.request({ type: "ListOutbox" }, "Outbox");
    expect(entries[0]).toMatchObject({ state: "failed", error: "rejected", attempts: 1 });
  });

  it("keeps messages scheduled months ahead waiting (no timer overflow)", async () => {
    vi.useFakeTimers();
    const { client } = setup();
    const { draft_id } = await client.request({ type: "SaveDraft", draft: draft() }, "DraftSaved");
    const day = 86_400_000;
    await client.request(
      {
        type: "SendDraft",
        draft_id,
        delay_secs: null,
        send_at: new Date(Date.now() + 60 * day).toISOString(),
      },
      "SendQueued",
    );
    await vi.advanceTimersByTimeAsync(30 * day);
    expect((await client.request({ type: "ListOutbox" }, "Outbox")).entries[0]?.state).toBe(
      "scheduled",
    );
    await vi.advanceTimersByTimeAsync(31 * day);
    expect((await client.request({ type: "ListOutbox" }, "Outbox")).entries).toEqual([]);
  });

  it("validates drafts", async () => {
    const { client } = setup();
    expect(
      await client.call({ type: "SaveDraft", draft: draft({ identity_id: "nobody@x.example" }) }),
    ).toMatchObject({ code: "invalid_request" });
    expect(
      await client.call({ type: "SaveDraft", draft: draft({ html: "<p>x</p>" }) }),
    ).toMatchObject({
      code: "unsupported",
    });
    const { draft_id } = await client.request(
      { type: "SaveDraft", draft: draft({ to: [] }) },
      "DraftSaved",
    );
    expect(
      await client.call({ type: "SendDraft", draft_id, delay_secs: 0, send_at: null }),
    ).toMatchObject({ code: "invalid_request" });
  });
});
