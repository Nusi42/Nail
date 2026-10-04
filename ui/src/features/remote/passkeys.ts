/**
 * Passkeys in the browser (WebAuthn through `navigator.credentials`,
 * wrapped by `@simplewebauthn/browser`, MIT). The login page gets a
 * `PasskeyApi` so tests (and Playwright's virtual authenticator in WP45)
 * can drive it; `browserPasskeys` is the real one.
 */
import {
  WebAuthnError,
  browserSupportsWebAuthn,
  startAuthentication,
  startRegistration,
  type PublicKeyCredentialCreationOptionsJSON,
  type PublicKeyCredentialRequestOptionsJSON,
} from "@simplewebauthn/browser";

export interface PasskeyApi {
  /** Whether this browser can use passkeys at all. */
  supported: () => boolean;
  /** Runs an assertion; resolves with `PublicKeyCredential.toJSON()`. */
  authenticate: (options: unknown) => Promise<unknown>;
  /** Runs a registration; resolves with `PublicKeyCredential.toJSON()`. */
  register: (options: unknown) => Promise<unknown>;
}

/** The server sends `{ publicKey: … }` or the bare options JSON. */
function unwrap(options: unknown): unknown {
  if (options !== null && typeof options === "object" && "publicKey" in options) {
    return options.publicKey;
  }
  return options;
}

export const browserPasskeys: PasskeyApi = {
  supported: () => browserSupportsWebAuthn(),
  authenticate: (options) =>
    startAuthentication({
      optionsJSON: unwrap(options) as PublicKeyCredentialRequestOptionsJSON,
    }),
  register: (options) =>
    startRegistration({
      optionsJSON: unwrap(options) as PublicKeyCredentialCreationOptionsJSON,
    }),
};

/** Why a passkey ceremony did not produce a credential. */
export type PasskeyProblem = "cancelled" | "not-supported" | "failed";

/**
 * Classifies a ceremony error: the user closing the prompt (or it timing
 * out) is `cancelled` and not worth an alarming message.
 */
export function passkeyProblem(error: unknown): PasskeyProblem {
  if (error instanceof WebAuthnError) {
    if (error.code === "ERROR_CEREMONY_ABORTED") return "cancelled";
    const cause: unknown = error.cause;
    if (cause !== null && typeof cause === "object" && "name" in cause) {
      if (cause.name === "NotAllowedError" || cause.name === "AbortError") return "cancelled";
    }
    if (error.code === "ERROR_INVALID_DOMAIN" || error.code === "ERROR_INVALID_RP_ID") {
      return "not-supported";
    }
    return "failed";
  }
  // `DOMException` is not an `Error` everywhere: look at the name only.
  const name =
    error !== null && typeof error === "object" && "name" in error ? String(error.name) : "";
  if (name === "NotAllowedError" || name === "AbortError") return "cancelled";
  if (name === "NotSupportedError" || name === "SecurityError") return "not-supported";
  return "failed";
}
