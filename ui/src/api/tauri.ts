/**
 * Transport for the desktop shell: one Tauri command (`nail_call`) plus the
 * `nail://event` event channel (ADR 0002).
 */
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { ClosableTransport, NailError } from "./client";
import type { Event as NailEvent, Request, Response } from "./generated";

/** Name of the Tauri command that handles every request. */
export const TAURI_COMMAND = "nail_call";
/** Name of the Tauri event that carries core events. */
export const TAURI_EVENT = "nail://event";

/** The two Tauri functions the transport needs (injectable for tests). */
export interface TauriApi {
  invoke: <T>(cmd: string, args: Record<string, unknown>) => Promise<T>;
  listen: (event: string, handler: (event: { payload: unknown }) => void) => Promise<() => void>;
}

const defaultApi: TauriApi = {
  invoke: (cmd, args) => invoke(cmd, args),
  listen: (event, handler) => listen(event, handler),
};

export class TauriTransport extends ClosableTransport {
  private readonly api: TauriApi;
  private unlisten: (() => void) | null = null;

  constructor(api: TauriApi = defaultApi) {
    super();
    this.api = api;
    void this.api
      .listen(TAURI_EVENT, (event) => {
        // The core emits protocol events on this channel only.
        this.events.emit(event.payload as NailEvent);
      })
      .then((unlisten) => {
        if (this.closed) {
          unlisten();
        } else {
          this.unlisten = unlisten;
        }
      })
      .catch((error: unknown) => {
        console.error("nail: cannot listen for core events", error);
      });
  }

  async send(request: Request): Promise<Response> {
    if (this.closed) {
      throw new NailError("unavailable", "transport closed");
    }
    try {
      return await this.api.invoke<Response>(TAURI_COMMAND, { request });
    } catch (error) {
      throw new NailError("internal", `nail_call failed: ${String(error)}`);
    }
  }

  protected override release(): void {
    this.unlisten?.();
    this.unlisten = null;
  }
}
