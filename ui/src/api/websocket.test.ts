import { NailError, TransportClient, type ConnectionStatus } from "./client";
import type { Event as NailEvent, RpcRequest, ServerMessage } from "./generated";
import {
  WebSocketTransport,
  backoffDelay,
  defaultWebSocketUrl,
  type WebSocketLike,
} from "./websocket";
import { must } from "../test/must";

class FakeSocket implements WebSocketLike {
  static instances: FakeSocket[] = [];
  readyState = 0;
  sent: RpcRequest[] = [];
  closed = false;
  onopen: ((event: unknown) => void) | null = null;
  onmessage: ((event: { data: unknown }) => void) | null = null;
  onclose: ((event: unknown) => void) | null = null;
  onerror: ((event: unknown) => void) | null = null;
  readonly url: string;

  constructor(url: string) {
    this.url = url;
    FakeSocket.instances.push(this);
  }

  send(data: string): void {
    this.sent.push(JSON.parse(data) as RpcRequest);
  }

  close(): void {
    this.closed = true;
    this.readyState = 3;
  }

  // Test helpers (the "server side").
  open(): void {
    this.readyState = 1;
    this.onopen?.({});
  }

  push(message: ServerMessage | string): void {
    this.onmessage?.({ data: typeof message === "string" ? message : JSON.stringify(message) });
  }

  drop(): void {
    this.readyState = 3;
    this.onerror?.({});
    this.onclose?.({});
  }
}

const latest = () => must(FakeSocket.instances[FakeSocket.instances.length - 1]);

function create() {
  return new WebSocketTransport({
    url: "ws://nail.test/api/ws",
    createSocket: (url) => new FakeSocket(url),
    initialBackoffMs: 100,
    maxBackoffMs: 1000,
  });
}

beforeEach(() => {
  FakeSocket.instances = [];
  vi.useFakeTimers();
});

afterEach(() => {
  vi.useRealTimers();
});

describe("WebSocketTransport", () => {
  it("derives the endpoint from the page origin", () => {
    expect(defaultWebSocketUrl({ protocol: "http:", host: "127.0.0.1:7777" })).toBe(
      "ws://127.0.0.1:7777/api/ws",
    );
    expect(defaultWebSocketUrl({ protocol: "https:", host: "nail.example" })).toBe(
      "wss://nail.example/api/ws",
    );
  });

  it("queues requests until the socket opens, then correlates responses by id", async () => {
    const transport = create();
    const client = new TransportClient(transport);
    const socket = latest();
    expect(socket.url).toBe("ws://nail.test/api/ws");

    const pong = client.request({ type: "Ping" }, "Pong");
    const accounts = client.request({ type: "ListAccounts" }, "Accounts");
    expect(socket.sent).toEqual([]);

    socket.open();
    expect(socket.sent.map((f) => f.request.type)).toEqual(["Ping", "ListAccounts"]);
    const [ping, list] = socket.sent as [RpcRequest, RpcRequest];
    expect(ping.id).not.toBe(list.id);

    // Answer out of order.
    socket.push({ kind: "response", id: list.id, response: { type: "Accounts", accounts: [] } });
    socket.push({
      kind: "response",
      id: ping.id,
      response: { type: "Pong", version: "1", protocol: 1 },
    });
    await expect(pong).resolves.toEqual({ type: "Pong", version: "1", protocol: 1 });
    await expect(accounts).resolves.toEqual({ type: "Accounts", accounts: [] });
    expect(transport.status()).toBe("connected");
  });

  it("turns Error responses into NailError", async () => {
    const client = new TransportClient(create());
    const socket = latest();
    socket.open();
    const result = client.request({ type: "GetMessage", id: "x" }, "Message");
    const id = must(socket.sent[0]).id;
    socket.push({
      kind: "response",
      id,
      response: { type: "Error", code: "not_found", message: "gone" },
    });
    await expect(result).rejects.toMatchObject({ name: "NailError", code: "not_found" });
  });

  it("delivers pushed events to subscribers", () => {
    const transport = create();
    const socket = latest();
    socket.open();
    const events: NailEvent[] = [];
    const off = transport.onEvent((e) => events.push(e));
    socket.push({ kind: "event", event: { type: "MailChanged", folder_ids: ["f1"] } });
    off();
    socket.push({ kind: "event", event: { type: "MailChanged", folder_ids: ["f2"] } });
    expect(events).toEqual([{ type: "MailChanged", folder_ids: ["f1"] }]);
  });

  it("ignores malformed frames", () => {
    const transport = create();
    const socket = latest();
    socket.open();
    const warn = vi.spyOn(console, "warn").mockReturnValue(undefined);
    const events: NailEvent[] = [];
    transport.onEvent((e) => events.push(e));
    socket.push("not json");
    socket.onmessage?.({ data: new ArrayBuffer(2) });
    expect(events).toEqual([]);
    expect(warn).toHaveBeenCalledTimes(2);
    warn.mockRestore();
  });

  it("ignores frames of an unknown kind and non-object frames", async () => {
    const transport = create();
    const socket = latest();
    socket.open();
    const warn = vi.spyOn(console, "warn").mockReturnValue(undefined);
    const events: NailEvent[] = [];
    const listener = vi.fn((e: NailEvent) => events.push(e));
    transport.onEvent(listener);
    socket.push(JSON.stringify({ kind: "future-frame", payload: { x: 1 } }));
    socket.push(JSON.stringify({ event: { type: "MailChanged", folder_ids: ["f1"] } }));
    socket.push("null");
    socket.push("42");
    expect(listener).not.toHaveBeenCalled();
    expect(warn).toHaveBeenCalledTimes(4);
    warn.mockRestore();
    // The transport still works after the odd frames.
    const result = transport.send({ type: "ListAccounts" });
    const id = must(socket.sent[0]).id;
    socket.push({ kind: "response", id, response: { type: "Accounts", accounts: [] } });
    await expect(result).resolves.toEqual({ type: "Accounts", accounts: [] });
    socket.push({ kind: "event", event: { type: "MailChanged", folder_ids: ["f2"] } });
    expect(events).toEqual([{ type: "MailChanged", folder_ids: ["f2"] }]);
  });

  it("fails in-flight requests on disconnect and reconnects with backoff", async () => {
    const transport = create();
    const statuses: ConnectionStatus[] = [];
    transport.onStatus((s) => statuses.push(s));
    const first = latest();
    first.open();

    const inFlight = transport.send({ type: "Ping" });
    first.drop();
    await expect(inFlight).rejects.toBeInstanceOf(NailError);
    await expect(inFlight).rejects.toMatchObject({ code: "network" });
    expect(transport.status()).toBe("reconnecting");

    // Requests while disconnected are queued.
    const queued = transport.send({ type: "Ping" });

    expect(FakeSocket.instances).toHaveLength(1);
    vi.advanceTimersByTime(99);
    expect(FakeSocket.instances).toHaveLength(1);
    vi.advanceTimersByTime(1);
    expect(FakeSocket.instances).toHaveLength(2);

    // Second attempt fails too: the delay doubles.
    latest().drop();
    vi.advanceTimersByTime(199);
    expect(FakeSocket.instances).toHaveLength(2);
    vi.advanceTimersByTime(1);
    expect(FakeSocket.instances).toHaveLength(3);

    const third = latest();
    third.open();
    expect(statuses).toEqual(["connected", "reconnecting", "connected"]);
    const frame = must(third.sent[0]);
    expect(frame.request).toEqual({ type: "Ping" });
    third.push({
      kind: "response",
      id: frame.id,
      response: { type: "Pong", version: "1", protocol: 1 },
    });
    await expect(queued).resolves.toMatchObject({ type: "Pong" });

    // A successful connect resets the backoff.
    third.drop();
    vi.advanceTimersByTime(100);
    expect(FakeSocket.instances).toHaveLength(4);
  });

  it("caps the backoff", () => {
    expect(backoffDelay(0, 500, 10_000)).toBe(500);
    expect(backoffDelay(3, 500, 10_000)).toBe(4000);
    expect(backoffDelay(10, 500, 10_000)).toBe(10_000);
    expect(backoffDelay(1000, 500, 10_000)).toBe(10_000);
  });

  it("close() rejects pending requests and stops reconnecting", async () => {
    const transport = create();
    const socket = latest();
    const queued = transport.send({ type: "Ping" });
    transport.close();
    await expect(queued).rejects.toMatchObject({ code: "unavailable" });
    await expect(transport.send({ type: "Ping" })).rejects.toMatchObject({ code: "unavailable" });
    expect(socket.closed).toBe(true);
    vi.advanceTimersByTime(60_000);
    expect(FakeSocket.instances).toHaveLength(1);
    expect(transport.status()).toBe("closed");
  });
});
