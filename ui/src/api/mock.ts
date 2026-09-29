/**
 * In-memory transport answering every M0 request from `MockData`.
 * Used by unit tests and by `?mock=1` in the dev server.
 */
import { Listeners, type ConnectionStatus, type Transport } from "./client";
import type { Event as NailEvent, Request, Response } from "./generated";
import { createMockData, type MockData } from "./mockData";

export interface MockTransportOptions {
  data?: MockData;
  /** Artificial latency per request in ms (0 = next microtask). */
  latencyMs?: number;
}

export class MockTransport implements Transport {
  readonly data: MockData;
  /** Every request received, in order (for assertions). */
  readonly requests: Request[] = [];
  private readonly latencyMs: number;
  private readonly events = new Listeners<NailEvent>();
  private readonly statuses = new Listeners<ConnectionStatus>();
  private closed = false;

  constructor(options: MockTransportOptions = {}) {
    this.data = options.data ?? createMockData();
    this.latencyMs = options.latencyMs ?? 0;
  }

  async send(request: Request): Promise<Response> {
    this.requests.push(request);
    if (this.latencyMs > 0) {
      await new Promise((resolve) => setTimeout(resolve, this.latencyMs));
    }
    return this.answer(request);
  }

  /** Pushes an event to all subscribers, as the core would. */
  emit(event: NailEvent): void {
    this.events.emit(event);
  }

  onEvent(listener: (event: NailEvent) => void): () => void {
    return this.events.add(listener);
  }

  onStatus(listener: (status: ConnectionStatus) => void): () => void {
    return this.statuses.add(listener);
  }

  status(): ConnectionStatus {
    return this.closed ? "closed" : "connected";
  }

  close(): void {
    this.closed = true;
    this.statuses.emit("closed");
  }

  private answer(request: Request): Response {
    const d = this.data;
    if (this.closed) {
      return { type: "Error", code: "unavailable", message: "mock transport closed" };
    }
    switch (request.type) {
      case "Ping":
        return { type: "Pong", version: "0.0.0-mock", protocol: 1 };
      case "GetConfig":
        return { type: "Config", theme: d.theme, layout: d.layout, locale: d.locale };
      case "ListAccounts":
        return { type: "Accounts", accounts: d.accounts };
      case "ListFolders":
        if (!d.accounts.some((a) => a.id === request.account_id)) {
          return notFound(`account ${request.account_id}`);
        }
        return {
          type: "Folders",
          folders: d.folders.filter((f) => f.account_id === request.account_id),
        };
      case "ListMessages": {
        if (!d.folders.some((f) => f.id === request.folder_id)) {
          return notFound(`folder ${request.folder_id}`);
        }
        const all = d.messages.filter((m) => m.folder_id === request.folder_id);
        return {
          type: "Messages",
          folder_id: request.folder_id,
          offset: request.offset,
          total: all.length,
          messages: all.slice(request.offset, request.offset + request.limit),
        };
      }
      case "GetMessage": {
        const body = d.bodies.get(request.id);
        return body ? { type: "Message", message: body } : notFound(`message ${request.id}`);
      }
      case "SyncNow": {
        const account = request.account_id ?? d.accounts[0]?.id ?? "";
        queueMicrotask(() => {
          this.emit({ type: "SyncProgress", account_id: account, done: 0, total: 1 });
          this.emit({ type: "SyncProgress", account_id: account, done: 1, total: 1 });
        });
        return { type: "SyncStarted" };
      }
    }
  }
}

function notFound(what: string): Response {
  return { type: "Error", code: "not_found", message: `${what} not found` };
}
