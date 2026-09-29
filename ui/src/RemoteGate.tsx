/**
 * The remote UI's login gate (web mode; ADR 0041). Before the app opens
 * its WebSocket it asks `POST /api/auth` for the browser's status:
 *
 * - no such route (the local, loopback `nail serve`): the app opens as
 *   before, without a login;
 * - a valid session: the app opens;
 * - otherwise the login page (`RemoteLogin`: pairing, passkey, password +
 *   code) is shown until the browser has a session.
 *
 * While the app runs, a session that ends (logout, revoked, expired) brings
 * the login page back, and `reauth_required` answers show `RemoteReauth`
 * (see `api/reauth.ts`).
 */
import { useStore } from "zustand";
import type { AuthStatus } from "./api/generated";
import { reauthStore } from "./api/reauth";
import {
  RemoteLogin,
  RemoteReauth,
  remoteAuth,
  remoteStore,
  useRemoteState,
} from "./features/remote";
import { AuthTransportError } from "./features/remote/authClient";

/** What the gate decided. */
export type GateState = "checking" | "open" | "login";

/** The login methods of the last status (re-authentication offers them). */
let lastStatus: AuthStatus | null = null;

/**
 * Asks the server whether this browser needs to sign in. `open` for the
 * local UI (no login route) and for a browser with a session.
 */
export async function checkRemoteGate(): Promise<GateState> {
  try {
    const answer = await remoteAuth().transport.send({ type: "Status" });
    if (answer.type !== "Status") return "login";
    lastStatus = answer.status;
    if (answer.status.authenticated && answer.status.session !== null) {
      remoteStore.getState().setSession(answer.status.session);
      return "open";
    }
    return "login";
  } catch (error) {
    // No login route: the local UI of `nail serve` (loopback, no login).
    if (error instanceof AuthTransportError && error.status === 404) return "open";
    return "login";
  }
}

/** The login page; `onLoggedIn` once the browser has a session. */
export function RemoteLoginPage({ onLoggedIn }: { onLoggedIn: () => void }) {
  return (
    <RemoteLogin
      auth={remoteAuth().transport}
      passkeys={remoteAuth().passkeys}
      onLoggedIn={(session) => {
        remoteStore.getState().setSession(session);
        onLoggedIn();
      }}
    />
  );
}

/** `RemoteReauth` while a request waits for a re-authentication. */
export function ReauthPrompt() {
  const pending = useStore(reauthStore, (s) => s.pending);
  const session = useRemoteState((s) => s.session);
  if (!pending || session === null) return null;
  const finish = (ok: boolean) => reauthStore.getState().finish(ok);
  return (
    <div data-part="remote-reauth.overlay" role="dialog" aria-modal="true">
      <RemoteReauth
        auth={remoteAuth().transport}
        passkeys={remoteAuth().passkeys}
        userName={session.user_name}
        passkeyAvailable={lastStatus?.methods.passkey ?? false}
        onDone={() => finish(true)}
        onCancel={() => finish(false)}
      />
    </div>
  );
}
