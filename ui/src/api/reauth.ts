/**
 * Re-authentication in the remote UI (ADR 0041): the core answers an
 * admin or sensitive request of a session that has not signed in recently
 * with `reauth_required`. This transport wrapper then asks the user to
 * confirm who they are (the app shell shows `RemoteReauth` while
 * {@link reauthStore} is pending) and sends the request once more; when
 * the user cancels, the original error stands.
 */
import { createStore } from "zustand/vanilla";
import type { ConnectionStatus, Transport } from "./client";
import type { Event as NailEvent, Request, Response } from "./generated";

export interface ReauthState {
  /** Whether a request waits for the user to re-authenticate. */
  pending: boolean;
  waiters: ((ok: boolean) => void)[];
  /** Resolves `true` after a re-authentication, `false` when cancelled. */
  request: () => Promise<boolean>;
  finish: (ok: boolean) => void;
}

export const reauthStore = createStore<ReauthState>()((set, get) => ({
  pending: false,
  waiters: [],
  request: () =>
    new Promise<boolean>((resolve) => {
      set((s) => ({ pending: true, waiters: [...s.waiters, resolve] }));
    }),
  finish: (ok) => {
    const { waiters } = get();
    set({ pending: false, waiters: [] });
    for (const waiter of waiters) waiter(ok);
  },
}));

/** Retries requests answered `reauth_required` after a re-authentication. */
export class ReauthTransport implements Transport {
  constructor(private readonly inner: Transport) {}

  async send(request: Request): Promise<Response> {
    const answer = await this.inner.send(request);
    if (answer.type !== "Error" || answer.code !== "reauth_required") return answer;
    const ok = await reauthStore.getState().request();
    return ok ? this.inner.send(request) : answer;
  }

  onEvent(listener: (event: NailEvent) => void): () => void {
    return this.inner.onEvent(listener);
  }

  onStatus(listener: (status: ConnectionStatus) => void): () => void {
    return this.inner.onStatus(listener);
  }

  status(): ConnectionStatus {
    return this.inner.status();
  }

  close(): void {
    this.inner.close();
  }
}
