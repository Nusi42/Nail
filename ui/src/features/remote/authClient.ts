/**
 * The browser's side of the login protocol (ADR 0036): `POST /api/auth`
 * with an `AuthRequest`, answered with an `AuthResponse`. It runs before
 * there is a session, so it does not go through `NailClient`.
 *
 * Cookies (`__Host-nail-session`, `__Host-nail-device`) are `HttpOnly` and
 * handled by the browser; this module only carries the CSRF token that
 * `Status` hands out in the `x-nail-csrf` header of every other request.
 */
import {
  AUTH_PATH,
  type AuthRequest,
  type AuthResponse,
  CSRF_HEADER,
  NO_LOGIN_STATUS,
} from "../../api/generated";
export { AUTH_PATH, CSRF_HEADER, NO_LOGIN_STATUS } from "../../api/generated";

/** Sends login-protocol requests. */
export interface AuthTransport {
  send: (request: AuthRequest) => Promise<AuthResponse>;
}

/** The server could not be reached or did not answer with the protocol. */
export class AuthTransportError extends Error {
  readonly status: number | null;

  constructor(status: number | null, message: string) {
    super(message);
    this.name = "AuthTransportError";
    this.status = status;
  }
}

type Fetch = (input: string, init: RequestInit) => Promise<Response>;

/** The login protocol over `fetch` (same origin, cookies included). */
export class HttpAuthTransport implements AuthTransport {
  private csrf: string | null = null;
  private readonly fetchImpl: Fetch;
  private readonly base: string;

  constructor(options: { fetch?: Fetch; base?: string } = {}) {
    this.fetchImpl = options.fetch ?? ((input, init) => globalThis.fetch(input, init));
    this.base = options.base ?? "";
  }

  async send(request: AuthRequest): Promise<AuthResponse> {
    if (request.type !== "Status" && this.csrf === null) {
      await this.post({ type: "Status" });
    }
    const answer = await this.post(request);
    // Logging in or out may rotate the token: fetch it again next time.
    if (answer.type === "LoggedIn" || answer.type === "LoggedOut") this.csrf = null;
    return answer;
  }

  private async post(request: AuthRequest): Promise<AuthResponse> {
    const headers: Record<string, string> = { "content-type": "application/json" };
    if (request.type !== "Status" && this.csrf !== null) headers[CSRF_HEADER] = this.csrf;
    let response: Response;
    try {
      response = await this.fetchImpl(`${this.base}${AUTH_PATH}`, {
        method: "POST",
        credentials: "same-origin",
        cache: "no-store",
        headers,
        body: JSON.stringify(request),
      });
    } catch (e) {
      throw new AuthTransportError(null, e instanceof Error ? e.message : String(e));
    }
    if (response.status === NO_LOGIN_STATUS) {
      throw new AuthTransportError(NO_LOGIN_STATUS, `${AUTH_PATH}: this server has no login`);
    }
    if (!response.ok) {
      throw new AuthTransportError(
        response.status,
        `${AUTH_PATH} answered ${String(response.status)}`,
      );
    }
    const answer = (await response.json()) as AuthResponse;
    if (answer.type === "Status") this.csrf = answer.status.csrf_token;
    return answer;
  }
}
