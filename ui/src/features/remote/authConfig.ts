/**
 * Which login-protocol transport and WebAuthn implementation the remote
 * feature uses outside the login page (the `remote.logout` action). The
 * app shell configures it in web mode; the default talks to
 * `POST /api/auth` on the page's origin with the browser's WebAuthn.
 */
import { HttpAuthTransport, type AuthTransport } from "./authClient";
import { browserPasskeys, type PasskeyApi } from "./passkeys";

export interface RemoteAuthConfig {
  transport: AuthTransport;
  passkeys: PasskeyApi;
}

let config: RemoteAuthConfig | null = null;

/** The configured transport and passkeys (created on first use). */
export function remoteAuth(): RemoteAuthConfig {
  config ??= { transport: new HttpAuthTransport(), passkeys: browserPasskeys };
  return config;
}

/** Replaces the transport and / or passkeys (`null` resets to the defaults). */
export function configureRemoteAuth(next: Partial<RemoteAuthConfig> | null): void {
  config = next === null ? null : { ...remoteAuth(), ...next };
}
