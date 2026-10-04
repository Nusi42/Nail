import type { Event as NailEvent, Request, Response } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import {
  HttpAuthTransport,
  AUTH_PATH,
  AuthTransportError,
  CSRF_HEADER,
  NO_LOGIN_STATUS,
} from "./authClient";
import { MOCK_MAX_FAILURES, MOCK_TOTP_CODE } from "./mock";
import { MockAuthServer } from "./mockAuth";
import { presetGrant } from "./model";

const PASSWORD = "correct horse battery";

async function setup(options: { totp?: boolean } = {}) {
  const core = new MockTransport();
  const events: NailEvent[] = [];
  core.onEvent((e) => events.push(e));
  const send = async <T extends Response["type"]>(request: Request, type: T) => {
    const answer = await core.send(request);
    if (answer.type !== type) throw new Error(JSON.stringify(answer));
    return answer as Extract<Response, { type: T }>;
  };
  const { user } = await send(
    {
      type: "CreateRemoteUser",
      name: "ada",
      display_name: null,
      grant: presetGrant("full", { admin: true }),
    },
    "RemoteUserSaved",
  );
  await send(
    { type: "SetRemotePassword", user_id: user.id, password: PASSWORD },
    "RemoteUserSaved",
  );
  if (options.totp) {
    await send({ type: "StartTotpEnrollment", user_id: user.id }, "TotpEnrollment");
    await send(
      { type: "ConfirmTotpEnrollment", user_id: user.id, code: MOCK_TOTP_CODE },
      "RemoteUserSaved",
    );
  }
  const pairingCode = async () =>
    (
      await send(
        {
          type: "CreatePairingCode",
          user_id: user.id,
          device_name: null,
          grant: presetGrant("read-only"),
          ttl_minutes: null,
        },
        "PairingCodeCreated",
      )
    ).code;
  return { core, events, send, user, pairingCode, s: core.data.features.remote };
}

describe("mock login protocol", () => {
  it("pairs a browser once with a code, then signs in with the password", async () => {
    const ctx = await setup();
    const browser = new MockAuthServer(ctx.core);
    const status = await browser.send({ type: "Status" });
    expect(status).toMatchObject({
      type: "Status",
      status: { authenticated: false, paired: false },
    });
    expect(
      await browser.send({
        type: "Password",
        user_name: "ada",
        password: PASSWORD,
        totp_code: null,
        purpose: "login",
      }),
    ).toEqual({ type: "Failed", reason: "not-paired", retry_after_secs: null });

    const code = await ctx.pairingCode();
    const paired = await browser.send({
      type: "Pair",
      code: code.toLowerCase().replace("-", " "),
      device_name: "Phone",
    });
    expect(paired).toMatchObject({
      type: "LoggedIn",
      session: { auth_method: "pairing", device_name: "Phone", user_name: "ada" },
    });
    if (paired.type !== "LoggedIn") throw new Error("not logged in");
    // The session never exceeds the device (read-only) or the user.
    expect(paired.session.grant).toEqual(presetGrant("read-only"));
    expect(ctx.events).toContainEqual({
      type: "RemoteSecurityAlert",
      alert: {
        kind: "new-device",
        device_id: paired.session.device_id,
        device_name: "Phone",
        user_name: "ada",
        ip: "192.168.1.42",
      },
    });

    // The code is single-use.
    const other = new MockAuthServer(ctx.core, { ip: "192.168.1.43" });
    expect(await other.send({ type: "Pair", code, device_name: "Laptop" })).toMatchObject({
      type: "Failed",
      reason: "invalid-code",
    });

    await browser.send({ type: "Logout" });
    expect(ctx.events).toContainEqual({
      type: "RemoteSessionEnded",
      session_id: paired.session.id,
      reason: "logout",
    });
    const again = await browser.send({
      type: "Password",
      user_name: "ADA",
      password: PASSWORD,
      totp_code: null,
      purpose: "login",
    });
    expect(again).toMatchObject({ type: "LoggedIn", session: { auth_method: "password" } });
    expect(ctx.s.audit.map((e) => `${e.kind}:${e.outcome}`)).toEqual([
      "password-changed:success",
      "pairing-created:success",
      "pairing:success",
      "login:success",
      "pairing:failure",
      "logout:success",
      "login:success",
    ]);
  });

  it("asks for the TOTP code and rejects a wrong one", async () => {
    const ctx = await setup({ totp: true });
    const browser = new MockAuthServer(ctx.core);
    await browser.send({ type: "Pair", code: await ctx.pairingCode(), device_name: "Phone" });
    await browser.send({ type: "Logout" });
    const login = {
      type: "Password" as const,
      user_name: "ada",
      password: PASSWORD,
      purpose: "login" as const,
    };
    expect(await browser.send({ ...login, totp_code: null })).toEqual({ type: "TotpRequired" });
    expect(await browser.send({ ...login, totp_code: "999999" })).toMatchObject({
      type: "Failed",
      reason: "invalid-code",
    });
    expect(await browser.send({ ...login, totp_code: MOCK_TOTP_CODE })).toMatchObject({
      type: "LoggedIn",
      session: { auth_method: "password-totp" },
    });
  });

  it("locks the name out after repeated failures and raises an alert", async () => {
    const ctx = await setup();
    const browser = new MockAuthServer(ctx.core);
    await browser.send({ type: "Pair", code: await ctx.pairingCode(), device_name: "Phone" });
    const attempt = (password: string) =>
      browser.send({
        type: "Password",
        user_name: "ada",
        password,
        totp_code: null,
        purpose: "login",
      });
    for (let i = 1; i < MOCK_MAX_FAILURES; i++) {
      expect(await attempt(`wrong ${String(i)}`)).toMatchObject({ reason: "invalid-credentials" });
    }
    expect(await attempt("wrong again")).toMatchObject({
      type: "Failed",
      reason: "locked-out",
      retry_after_secs: 300,
    });
    // Even the right password is refused while locked.
    const locked = await attempt(PASSWORD);
    expect(locked).toMatchObject({ type: "Failed", reason: "locked-out" });
    expect(
      ctx.events.filter((e) => e.type === "RemoteSecurityAlert").map((e) => e.alert.kind),
    ).toContain("lockout");
    await ctx.send({ type: "UnlockRemoteUser", user_id: ctx.user.id }, "RemoteUserSaved");
    delete ctx.s.lockedIps["192.168.1.42"];
    expect(await attempt(PASSWORD)).toMatchObject({ type: "LoggedIn" });
  });

  it("registers a passkey and signs in with it; challenges are single-use", async () => {
    const ctx = await setup();
    const browser = new MockAuthServer(ctx.core);
    expect(await browser.send({ type: "PasskeyRegisterStart", name: "Phone" })).toMatchObject({
      type: "Failed",
      reason: "not-logged-in",
    });
    await browser.send({ type: "Pair", code: await ctx.pairingCode(), device_name: "Phone" });
    const start = await browser.send({ type: "PasskeyRegisterStart", name: "Phone key" });
    if (start.type !== "PasskeyChallenge") throw new Error(JSON.stringify(start));
    expect(start.options).toMatchObject({
      rp: { id: "nail-desktop.local" },
      user: { name: "ada" },
    });
    const registered = await browser.send({
      type: "PasskeyRegisterFinish",
      challenge_id: start.challenge_id,
      credential: { id: "cred-1", type: "public-key" },
    });
    expect(registered).toMatchObject({ type: "PasskeyRegistered", passkey: { name: "Phone key" } });
    await browser.send({ type: "Logout" });

    const login = await browser.send({ type: "PasskeyStart", user_name: "ada", purpose: "login" });
    if (login.type !== "PasskeyChallenge") throw new Error(JSON.stringify(login));
    expect(login.options).toMatchObject({ allowCredentials: [{ id: "cred-1" }] });
    const unknown = await browser.send({
      type: "PasskeyFinish",
      challenge_id: login.challenge_id,
      credential: { id: "cred-x" },
    });
    expect(unknown).toMatchObject({ type: "Failed", reason: "invalid-credentials" });
    // The challenge was used up by the failed attempt.
    expect(
      await browser.send({
        type: "PasskeyFinish",
        challenge_id: login.challenge_id,
        credential: { id: "cred-1" },
      }),
    ).toMatchObject({ type: "Failed", reason: "challenge-expired" });
    const next = await browser.send({ type: "PasskeyStart", user_name: null, purpose: "login" });
    if (next.type !== "PasskeyChallenge") throw new Error("no challenge");
    expect(
      await browser.send({
        type: "PasskeyFinish",
        challenge_id: next.challenge_id,
        credential: { id: "cred-1" },
      }),
    ).toMatchObject({ type: "LoggedIn", session: { auth_method: "passkey" } });

    // Re-authentication keeps the session and refreshes it.
    const re = await browser.send({ type: "PasskeyStart", user_name: "ada", purpose: "reauth" });
    if (re.type !== "PasskeyChallenge") throw new Error("no challenge");
    expect(
      await browser.send({
        type: "PasskeyFinish",
        challenge_id: re.challenge_id,
        credential: { id: "cred-1" },
      }),
    ).toMatchObject({ type: "Reauthenticated" });
  });

  it("ends the browser's session when its device is revoked", async () => {
    const ctx = await setup();
    const browser = new MockAuthServer(ctx.core);
    const paired = await browser.send({
      type: "Pair",
      code: await ctx.pairingCode(),
      device_name: "Phone",
    });
    if (paired.type !== "LoggedIn") throw new Error("not paired");
    await ctx.send({ type: "RevokeRemoteDevice", device_id: paired.session.device_id }, "Done");
    const status = await browser.send({ type: "Status" });
    expect(status).toMatchObject({ status: { authenticated: false, paired: false } });
  });
});

describe("HttpAuthTransport", () => {
  function fakeFetch(answers: unknown[]) {
    const calls: { url: string; init: RequestInit }[] = [];
    const fetch = (url: string, init: RequestInit) => {
      calls.push({ url, init });
      const next = answers.shift();
      if (next instanceof Error) return Promise.reject(next);
      if (typeof next === "number") {
        // A 204 answer has no body at all.
        const body = next === NO_LOGIN_STATUS ? null : "";
        return Promise.resolve(new Response(body, { status: next }));
      }
      return Promise.resolve(
        new Response(JSON.stringify(next), { headers: { "content-type": "application/json" } }),
      );
    };
    return { fetch, calls };
  }
  /** The `type` of the JSON request a fake fetch received. */
  const sentType = (init: RequestInit): string => {
    if (typeof init.body !== "string") throw new Error("no JSON body");
    return (JSON.parse(init.body) as { type: string }).type;
  };
  const status = (csrf: string) => ({
    type: "Status",
    status: {
      authenticated: false,
      paired: true,
      session: null,
      reauth_until: null,
      csrf_token: csrf,
      methods: { passkey: true, password: true },
      certificate_fingerprint: null,
    },
  });

  it("fetches the CSRF token first and sends it with every other request", async () => {
    const { fetch, calls } = fakeFetch([
      status("t1"),
      { type: "TotpRequired" },
      { type: "LoggedIn", session: {} },
      status("t2"),
      { type: "LoggedOut" },
    ]);
    const auth = new HttpAuthTransport({ fetch, base: "https://nail.local:4780" });
    const login = {
      type: "Password" as const,
      user_name: "ada",
      password: "x",
      totp_code: null,
      purpose: "login" as const,
    };
    expect(await auth.send(login)).toEqual({ type: "TotpRequired" });
    await auth.send({ ...login, totp_code: "123456" });
    await auth.send({ type: "Logout" });
    expect(calls.map((c) => sentType(c.init))).toEqual([
      "Status",
      "Password",
      "Password",
      "Status",
      "Logout",
    ]);
    expect(calls[0]?.url).toBe(`https://nail.local:4780${AUTH_PATH}`);
    const headers = calls.map((c) => (c.init.headers as Record<string, string>)[CSRF_HEADER]);
    expect(headers).toEqual([undefined, "t1", "t1", undefined, "t2"]);
    expect(
      calls.every((c) => c.init.method === "POST" && c.init.credentials === "same-origin"),
    ).toBe(true);
  });

  it("reports a server without a login (204) as an AuthTransportError", async () => {
    const { fetch } = fakeFetch([NO_LOGIN_STATUS]);
    const auth = new HttpAuthTransport({ fetch });
    await expect(auth.send({ type: "Status" })).rejects.toMatchObject({ status: NO_LOGIN_STATUS });
  });

  it("reports HTTP and network errors", async () => {
    const { fetch } = fakeFetch([500, new TypeError("offline")]);
    const auth = new HttpAuthTransport({ fetch });
    await expect(auth.send({ type: "Status" })).rejects.toMatchObject({ status: 500 });
    await expect(auth.send({ type: "Status" })).rejects.toBeInstanceOf(AuthTransportError);
  });
});
