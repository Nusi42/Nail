/**
 * Transport for `nail serve`: one WebSocket at `/api/ws` carrying
 * `RpcRequest` frames up and `ServerMessage` frames down (ADR 0002).
 * Reconnects with exponential backoff; requests issued while disconnected
 * are queued and sent after the next successful connect.
 */
import { Listeners, NailError, type ConnectionStatus, type Transport } from "./client";
import type { Event as NailEvent, Request, Response, RpcRequest, ServerMessage } from "./generated";

/** The subset of the DOM `WebSocket` the transport uses (fakeable in tests). */
export interface WebSocketLike {
  readonly readyState: number;
  send: (data: string) => void;
  close: (code?: number, reason?: string) => void;
  onopen: ((event: unknown) => void) | null;
  onmessage: ((event: { data: unknown }) => void) | null;
  onclose: ((event: unknown) => void) | null;
  onerror: ((event: unknown) => void) | null;
}

export interface WebSocketTransportOptions {
  /** Full WebSocket URL; defaults to `/api/ws` on the current origin. */
  url?: string;
  /** Socket factory; defaults to the browser's `WebSocket`. */
  createSocket?: (url: string) => WebSocketLike;
  /** First reconnect delay in ms (doubles per attempt). */
  initialBackoffMs?: number;
  /** Upper bound of the reconnect delay in ms. */
  maxBackoffMs?: number;
}

const OPEN = 1;

/** `ws(s)://<host>/api/ws` for the page's origin. */
export function defaultWebSocketUrl(location: Pick<Location, "protocol" | "host">): string {
  const scheme = location.protocol === "https:" ? "wss:" : "ws:";
  return `${scheme}//${location.host}/api/ws`;
}

/** Reconnect delay for the given (0-based) attempt. */
export function backoffDelay(attempt: number, initialMs: number, maxMs: number): number {
  return Math.min(maxMs, initialMs * 2 ** Math.min(attempt, 30));
}

interface Pending {
  resolve: (response: Response) => void;
  reject: (error: Error) => void;
}

export class WebSocketTransport implements Transport {
  private readonly url: string;
  private readonly createSocket: (url: string) => WebSocketLike;
  private readonly initialBackoffMs: number;
  private readonly maxBackoffMs: number;
  private readonly events = new Listeners<NailEvent>();
  private readonly statuses = new Listeners<ConnectionStatus>();
  /** Requests sent on the current socket, awaiting a response. */
  private readonly inFlight = new Map<number, Pending>();
  /** Requests waiting for a connection. */
  private outbox: { frame: RpcRequest; pending: Pending }[] = [];
  private socket: WebSocketLike | null = null;
  private nextId = 1;
  private attempt = 0;
  private reconnectTimer: ReturnType<typeof setTimeout> | null = null;
  private current: ConnectionStatus = "connecting";

  constructor(options: WebSocketTransportOptions = {}) {
    this.url = options.url ?? defaultWebSocketUrl(window.location);
    this.createSocket =
      options.createSocket ?? ((url) => new WebSocket(url) as unknown as WebSocketLike);
    this.initialBackoffMs = options.initialBackoffMs ?? 500;
    this.maxBackoffMs = options.maxBackoffMs ?? 10_000;
    this.connect();
  }

  send(request: Request): Promise<Response> {
    if (this.current === "closed") {
      return Promise.reject(new NailError("unavailable", "transport closed"));
    }
    return new Promise<Response>((resolve, reject) => {
      const frame: RpcRequest = { id: this.allocateId(), request };
      const pending = { resolve, reject };
      if (this.socket?.readyState === OPEN) {
        this.transmit(frame, pending);
      } else {
        this.outbox.push({ frame, pending });
      }
    });
  }

  onEvent(listener: (event: NailEvent) => void): () => void {
    return this.events.add(listener);
  }

  onStatus(listener: (status: ConnectionStatus) => void): () => void {
    return this.statuses.add(listener);
  }

  status(): ConnectionStatus {
    return this.current;
  }

  close(): void {
    if (this.current === "closed") {
      return;
    }
    this.setStatus("closed");
    if (this.reconnectTimer !== null) {
      clearTimeout(this.reconnectTimer);
      this.reconnectTimer = null;
    }
    const socket = this.socket;
    this.socket = null;
    if (socket) {
      socket.onopen = socket.onmessage = socket.onclose = socket.onerror = null;
      socket.close(1000, "client closed");
    }
    const error = new NailError("unavailable", "transport closed");
    this.failInFlight(error);
    for (const { pending } of this.outbox) {
      pending.reject(error);
    }
    this.outbox = [];
  }

  private allocateId(): number {
    const id = this.nextId;
    this.nextId = this.nextId >= Number.MAX_SAFE_INTEGER ? 1 : this.nextId + 1;
    return id;
  }

  private connect(): void {
    this.reconnectTimer = null;
    let socket: WebSocketLike;
    try {
      socket = this.createSocket(this.url);
    } catch (error) {
      console.error("nail: cannot open WebSocket", error);
      this.scheduleReconnect();
      return;
    }
    this.socket = socket;
    socket.onopen = () => {
      this.handleOpen();
    };
    socket.onmessage = (event) => {
      this.handleMessage(event.data);
    };
    socket.onclose = () => {
      this.handleClose(socket);
    };
    socket.onerror = () => {
      // A close event always follows; reconnecting is handled there.
    };
  }

  private handleOpen(): void {
    this.attempt = 0;
    this.setStatus("connected");
    const queued = this.outbox;
    this.outbox = [];
    for (const { frame, pending } of queued) {
      this.transmit(frame, pending);
    }
  }

  private transmit(frame: RpcRequest, pending: Pending): void {
    this.inFlight.set(frame.id, pending);
    try {
      this.socket?.send(JSON.stringify(frame));
    } catch (error) {
      this.inFlight.delete(frame.id);
      pending.reject(new NailError("network", `send failed: ${String(error)}`));
    }
  }

  private handleMessage(data: unknown): void {
    if (typeof data !== "string") {
      console.warn("nail: ignoring non-text WebSocket frame");
      return;
    }
    let message: ServerMessage;
    try {
      message = JSON.parse(data) as ServerMessage;
    } catch {
      console.warn("nail: ignoring malformed WebSocket frame");
      return;
    }
    if (message.kind === "response") {
      const pending = this.inFlight.get(message.id);
      if (pending) {
        this.inFlight.delete(message.id);
        pending.resolve(message.response);
      }
    } else {
      this.events.emit(message.event);
    }
  }

  private handleClose(socket: WebSocketLike): void {
    if (socket !== this.socket || this.current === "closed") {
      return;
    }
    this.socket = null;
    this.failInFlight(new NailError("network", "connection lost"));
    this.setStatus("reconnecting");
    this.scheduleReconnect();
  }

  private scheduleReconnect(): void {
    if (this.current === "closed") {
      return;
    }
    const delay = backoffDelay(this.attempt, this.initialBackoffMs, this.maxBackoffMs);
    this.attempt += 1;
    this.reconnectTimer = setTimeout(() => {
      this.connect();
    }, delay);
  }

  private failInFlight(error: Error): void {
    const pending = [...this.inFlight.values()];
    this.inFlight.clear();
    for (const entry of pending) {
      entry.reject(error);
    }
  }

  private setStatus(status: ConnectionStatus): void {
    if (this.current !== status) {
      this.current = status;
      this.statuses.emit(status);
    }
  }
}
