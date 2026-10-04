/**
 * Re-authentication of a remote session ("Confirm it’s you", ADR 0041) as
 * a window of the window manager (M5c): while a request waits for a fresh
 * sign-in (`api/reauth.ts`), the modal confirmation `remote-reauth` shows
 * `RemoteReauth`; closing the window (× or Escape) cancels, and the
 * request answers `reauth_required`.
 */
import { useEffect, useRef } from "react";
import { useStore } from "zustand";
import { reauthStore } from "../../api/reauth";
import { useUi, useUiStoreApi } from "../../state/uiStore";
import { remoteAuth } from "./authConfig";
import { RemoteReauth } from "./Login";
import { useRemoteState } from "./state";

/** The window kind. */
export const REAUTH_WINDOW = "remote-reauth";

/** Whether the server offers passkeys (from the gate's last status). */
let passkeyAvailable = false;

/** Records the login methods of the server's last status. */
export function setReauthMethods(passkey: boolean): void {
  passkeyAvailable = passkey;
}

/** The window's content. */
export function ReauthWindow() {
  const pending = useStore(reauthStore, (s) => s.pending);
  const session = useRemoteState((s) => s.session);
  if (!pending || session === null) return null;
  const finish = (ok: boolean) => {
    reauthStore.getState().finish(ok);
  };
  return (
    <RemoteReauth
      auth={remoteAuth().transport}
      passkeys={remoteAuth().passkeys}
      userName={session.user_name}
      passkeyAvailable={passkeyAvailable}
      onDone={() => {
        finish(true);
      }}
      onCancel={() => {
        finish(false);
      }}
    />
  );
}

/**
 * Keeps a waiting re-authentication and its window in step (mounted by the
 * app shell): a request that needs one opens the window, an answered one
 * closes it, and a window the user closed cancels.
 */
export function ReauthPrompt() {
  const store = useUiStoreApi();
  const pending = useStore(reauthStore, (s) => s.pending);
  const session = useRemoteState((s) => s.session);
  const open = useUi((s) => s.windows.includes(REAUTH_WINDOW));
  const shown = useRef(false);
  const wanted = pending && session !== null;
  useEffect(() => {
    if (wanted && !shown.current) {
      shown.current = true;
      store.getState().openWindow(REAUTH_WINDOW);
    } else if (!wanted && shown.current) {
      shown.current = false;
      if (store.getState().windows.includes(REAUTH_WINDOW)) {
        store.getState().closeWindow(REAUTH_WINDOW);
      }
    } else if (wanted && !open) {
      // Closed by the user: cancelled.
      shown.current = false;
      reauthStore.getState().finish(false);
    }
  }, [wanted, open, store]);
  return null;
}
