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
import { RemoteLogin, remoteAuth, remoteStore, setReauthMethods } from "./features/remote";
import { AuthTransportError } from "./features/remote/authClient";

/** What the gate decided. */
export type GateState = "checking" | "open" | "login";

/**
 * Asks the server whether this browser needs to sign in. `open` for the
 * local UI (no login route) and for a browser with a session.
 */
export async function checkRemoteGate(): Promise<GateState> {
  try {
    const answer = await remoteAuth().transport.send({ type: "Status" });
    if (answer.type !== "Status") return "login";
    // Re-authentication offers the same login methods.
    setReauthMethods(answer.status.methods.passkey);
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

/**
 * `RemoteReauth` while a request waits for a re-authentication: the modal
 * window `remote-reauth` (M5c, `features/remote/ReauthWindow.tsx`).
 */
export { ReauthPrompt } from "./features/remote";
