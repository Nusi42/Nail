/**
 * The renderer's only door to the core (design.md, "Architecture").
 *
 * A `NailClient` sends `Request`s and receives `Response`s and pushed
 * `Event`s. It is transport-agnostic: the desktop app uses Tauri IPC, the
 * browser a WebSocket, tests and `?mock=1` an in-memory mock.
 */
import type {
  ErrorCode,
  LocalizedMessage,
  Event as NailEvent,
  Request,
  Response,
} from "./generated";

/** Connection state of a transport, shown in the status bar. */
export type ConnectionStatus = "connecting" | "connected" | "reconnecting" | "closed";

/** The low-level channel a client talks through. */
export interface Transport {
  /** Sends one request and resolves with its response. */
  send(request: Request): Promise<Response>;
  /** Registers a listener for pushed events; returns an unsubscribe function. */
  onEvent(listener: (event: NailEvent) => void): () => void;
  /** Registers a listener for connection changes; returns an unsubscribe function. */
  onStatus(listener: (status: ConnectionStatus) => void): () => void;
  /** Current connection state. */
  status(): ConnectionStatus;
  /** Releases the channel; pending requests fail. */
  close(): void;
}

/** An error answered by the core, or a transport failure. */
export class NailError extends Error {
  readonly code: ErrorCode;
  /** The core's user-facing text, when it has a more specific one (ADR 0043). */
  readonly localized: LocalizedMessage | null;

  constructor(code: ErrorCode, message: string, localized: LocalizedMessage | null = null) {
    super(message);
    this.name = "NailError";
    this.code = code;
    this.localized = localized;
  }
}

/** Fluent message id for an error code (`not_found` → `error-not-found`). */
export function errorMessageId(code: ErrorCode): string {
  return `error-${code.replaceAll("_", "-")}`;
}

/** Fluent message id for any thrown value. */
export function errorMessageIdOf(error: unknown): string {
  return errorMessageId(error instanceof NailError ? error.code : "internal");
}

/**
 * The user-facing message of any thrown value: the core's `localized` text
 * when it sent one, else the text of the error code (ADR 0043).
 */
export function errorMessageOf(error: unknown): LocalizedMessage {
  if (error instanceof NailError && error.localized) return error.localized;
  return { id: errorMessageIdOf(error) };
}

type ResponseOf<T extends Response["type"]> = Extract<Response, { type: T }>;

/** The API the React tree uses. */
export interface NailClient {
  /** Sends a request and returns the raw response (which may be `Error`). */
  call(request: Request): Promise<Response>;
  /**
   * Sends a request and returns the response if it has the `expected` type.
   * Throws a `NailError` for `Error` responses and for unexpected types.
   */
  request<T extends Response["type"]>(request: Request, expected: T): Promise<ResponseOf<T>>;
  /** Listens for events pushed by the core. */
  subscribe(listener: (event: NailEvent) => void): () => void;
  /** Listens for connection changes. */
  onStatus(listener: (status: ConnectionStatus) => void): () => void;
  /** Current connection state. */
  status(): ConnectionStatus;
  /** Closes the underlying transport. */
  close(): void;
}

/** The standard `NailClient` on top of any `Transport`. */
export class TransportClient implements NailClient {
  readonly transport: Transport;

  constructor(transport: Transport) {
    this.transport = transport;
  }

  call(request: Request): Promise<Response> {
    return this.transport.send(request);
  }

  async request<T extends Response["type"]>(request: Request, expected: T): Promise<ResponseOf<T>> {
    const response = await this.transport.send(request);
    if (response.type === expected) {
      return response as ResponseOf<T>;
    }
    if (response.type === "Error") {
      throw new NailError(response.code, response.message, response.localized ?? null);
    }
    throw new NailError(
      "internal",
      `expected a ${expected} response to ${request.type}, got ${response.type}`,
    );
  }

  subscribe(listener: (event: NailEvent) => void): () => void {
    return this.transport.onEvent(listener);
  }

  onStatus(listener: (status: ConnectionStatus) => void): () => void {
    return this.transport.onStatus(listener);
  }

  status(): ConnectionStatus {
    return this.transport.status();
  }

  close(): void {
    this.transport.close();
  }
}

/** A tiny listener set used by the transports. */
export class Listeners<T> {
  private readonly set = new Set<(value: T) => void>();

  add(listener: (value: T) => void): () => void {
    this.set.add(listener);
    return () => {
      this.set.delete(listener);
    };
  }

  emit(value: T): void {
    for (const listener of [...this.set]) {
      try {
        listener(value);
      } catch (error) {
        console.error("nail: listener failed", error);
      }
    }
  }
}
