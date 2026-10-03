import defaultLayoutJson from "../../../crates/nail-api/fixtures/default-layout.json";
import { NailError, TransportClient, errorMessageId, errorMessageIdOf } from "./client";
import type { Event as NailEvent } from "./generated";
import { MockTransport } from "./mock";
import { createMockData } from "./mockData";
import { normalizeLayout, normalizeTheme } from "./normalize";
import { selectTransportKind } from "./select";
import { TAURI_COMMAND, TAURI_EVENT, TauriTransport, type TauriApi } from "./tauri";

describe("transport selection", () => {
  it("prefers ?mock=1, then Tauri, then WebSocket", () => {
    expect(selectTransportKind({ search: "?mock=1", hasTauri: true })).toBe("mock");
    expect(selectTransportKind({ search: "?lang=de&mock=true", hasTauri: false })).toBe("mock");
    expect(selectTransportKind({ search: "", hasTauri: true })).toBe("tauri");
    expect(selectTransportKind({ search: "?mock=0", hasTauri: false })).toBe("websocket");
  });
});

describe("MockTransport", () => {
  const client = () => new TransportClient(new MockTransport({ data: createMockData(40) }));

  it("serves accounts, folders, message pages and bodies", async () => {
    const c = client();
    const { accounts } = await c.request({ type: "ListAccounts" }, "Accounts");
    // The built-in "All accounts" comes first, without a name of its own.
    expect(accounts.map((a) => [a.id, a.name, a.kind])).toEqual([
      ["all", "", "virtual"],
      ["acc-personal", "Personal", "imap"],
      ["acc-work", "Work", "imap"],
    ]);
    const { folders } = await c.request(
      { type: "ListFolders", account_id: "acc-personal" },
      "Folders",
    );
    const inbox = folders.find((f) => f.role === "inbox");
    expect(inbox).toMatchObject({ total: 40, parent_id: null });
    expect(folders.find((f) => f.name === "Receipts")?.parent_id).toBe(inbox?.id);
    const page = await c.request(
      { type: "ListMessages", folder_id: inbox?.id ?? "", offset: 30, limit: 20 },
      "Messages",
    );
    expect(page).toMatchObject({ offset: 30, total: 40 });
    expect(page.messages).toHaveLength(10);
    const { message } = await c.request(
      { type: "GetMessage", id: page.messages[0]?.id ?? "" },
      "Message",
    );
    expect(message.headers.subject).toBe(page.messages[0]?.subject);
  });

  it("adds the security findings of the feature mock to message bodies", async () => {
    const transport = new MockTransport({ data: createMockData(5) });
    const c = new TransportClient(transport);
    const first = [...transport.data.bodies.keys()][0] ?? "";
    const plain = await c.request({ type: "GetMessage", id: first }, "Message");
    expect(plain.message.security).toBeUndefined();
    transport.data.features.security.messages[first] = {
      warnings: [{ kind: "suspicious-link", link: 0, href: "file:///c:/x" }],
    };
    const { message } = await c.request({ type: "GetMessage", id: first }, "Message");
    expect(message.warnings).toHaveLength(1);
  });

  it("answers the reference config", async () => {
    const config = await client().request({ type: "GetConfig" }, "Config");
    // M5b: one main window (rail + views); the mail view has two breakpoints.
    expect(config.layout.windows.main).toHaveLength(1);
    expect(config.layout.windows.mail).toHaveLength(2);
    expect(config.theme.parts.length).toBeGreaterThan(10);
  });

  it("answers the M1a requests of the contract", async () => {
    const { config } = await client().request({ type: "GetAccountsConfig" }, "AccountsConfig");
    expect(config.accounts.map((a) => a.id)).toEqual(["acc-personal", "acc-work"]);
    const response = await client().call({ type: "ListOutbox" });
    expect(response).toEqual({ type: "Outbox", entries: [] });
  });

  it("answers errors for unknown ids", async () => {
    await expect(
      client().request({ type: "GetMessage", id: "nope" }, "Message"),
    ).rejects.toMatchObject({
      code: "not_found",
    });
    await expect(
      client().request({ type: "ListFolders", account_id: "nope" }, "Folders"),
    ).rejects.toBeInstanceOf(NailError);
  });

  it("rejects unexpected response types", async () => {
    await expect(client().request({ type: "Ping" }, "Accounts")).rejects.toMatchObject({
      code: "internal",
    });
  });

  it("emits sync progress after SyncNow", async () => {
    const transport = new MockTransport();
    const events: NailEvent[] = [];
    transport.onEvent((e) => events.push(e));
    await expect(transport.send({ type: "SyncNow", account_id: null })).resolves.toEqual({
      type: "SyncStarted",
    });
    await Promise.resolve();
    expect(events.map((e) => e.type)).toEqual(["SyncProgress", "SyncProgress"]);
  });
});

describe("TauriTransport", () => {
  it("invokes nail_call and forwards nail://event", async () => {
    let handler: ((e: { payload: unknown }) => void) | null = null;
    const unlisten = vi.fn();
    const api: TauriApi = {
      invoke: vi.fn(async () => ({
        type: "Pong",
        version: "1",
        protocol: 1,
      })) as TauriApi["invoke"],
      listen: vi.fn(async (_name, h) => {
        handler = h as (e: { payload: unknown }) => void;
        return unlisten;
      }) as TauriApi["listen"],
    };
    const transport = new TauriTransport(api);
    await expect(transport.send({ type: "Ping" })).resolves.toMatchObject({ type: "Pong" });
    expect(api.invoke).toHaveBeenCalledWith(TAURI_COMMAND, { request: { type: "Ping" } });
    expect(api.listen).toHaveBeenCalledWith(TAURI_EVENT, expect.any(Function));

    await Promise.resolve();
    const events: NailEvent[] = [];
    transport.onEvent((e) => events.push(e));
    (handler as unknown as (e: { payload: unknown }) => void)({
      payload: { type: "MailChanged", folder_ids: [] },
    });
    expect(events).toEqual([{ type: "MailChanged", folder_ids: [] }]);
    transport.close();
    expect(unlisten).toHaveBeenCalled();
    await expect(transport.send({ type: "Ping" })).rejects.toMatchObject({ code: "unavailable" });
  });

  it("wraps invoke failures", async () => {
    const api: TauriApi = {
      invoke: (async () => {
        throw new Error("boom");
      }) as TauriApi["invoke"],
      listen: (async () => () => {}) as TauriApi["listen"],
    };
    await expect(new TauriTransport(api).send({ type: "Ping" })).rejects.toMatchObject({
      code: "internal",
    });
  });
});

describe("errors", () => {
  it("maps codes to Fluent ids", () => {
    expect(errorMessageId("not_found")).toBe("error-not-found");
    expect(errorMessageId("invalid_request")).toBe("error-invalid-request");
    expect(errorMessageIdOf(new NailError("auth_failed", ""))).toBe("error-auth-failed");
    expect(errorMessageIdOf(new Error("x"))).toBe("error-internal");
  });
});

describe("normalize", () => {
  it("fills omitted fields of the sparse fixtures", () => {
    const layout = normalizeLayout(defaultLayoutJson);
    const root = layout.windows.main?.[0]?.root;
    expect(root).toMatchObject({ type: "stack", label: null });
    expect(layout.actions["mail.next"]).toEqual({
      label: "@mail.action.next",
      icon: "chevron-down",
      alias_of: null,
      args: [],
    });
    expect(layout.keys[0]).toEqual({ context: "list", key: "j", action: "mail.next", args: [] });
    // Every default action has an icon since the M5a review; an omitted one is null.
    expect(normalizeLayout({ actions: { "x.y": { label: "@x" } } }).actions["x.y"]).toEqual({
      label: "@x",
      icon: null,
      alias_of: null,
      args: [],
    });
  });

  it("keeps the file positions of widgets (inspector)", () => {
    const layout = normalizeLayout({
      windows: {
        main: [
          {
            root: {
              type: "widget",
              kind: "folder-tree",
              source: { file: "layout.kdl", line: 4, column: 9 },
            },
          },
        ],
      },
    });
    expect(layout.windows.main?.[0]?.root).toMatchObject({
      type: "widget",
      source: { file: "layout.kdl", line: 4, column: 9 },
    });
    const bare = normalizeLayout({
      windows: { main: [{ root: { type: "widget", kind: "x", source: { line: 1 } } }] },
    });
    expect(bare.windows.main?.[0]?.root).toMatchObject({ source: null });
  });

  it("completes a theme", () => {
    const theme = normalizeTheme({ parts: [{ selector: "app" }], icons: { x: { path: "M0 0" } } });
    expect(theme.mode).toBe("system");
    expect(theme.parts[0]).toEqual({
      selector: "app",
      states: [],
      conditions: [],
      ancestors: [],
      declarations: {},
      source: null,
    });
    expect(theme.icons.x).toEqual({ path: "M0 0", view_box: "0 0 24 24" });
  });
});
