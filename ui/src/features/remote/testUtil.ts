/**
 * Test helpers of the remote feature: a remote user with a password, a
 * pairing code, a paired browser, and a fake WebAuthn.
 */
import type { Request, RemoteSession, Response } from "../../api/generated";
import type { MockTransport } from "../../api/mock";
import { MockAuthServer } from "./mockAuth";
import { presetGrant } from "./model";
import type { PasskeyApi } from "./passkeys";

export const PASSWORD = "correct horse battery";

export async function sendOk<T extends Response["type"]>(
  transport: MockTransport,
  request: Request,
  type: T,
): Promise<Extract<Response, { type: T }>> {
  const answer = await transport.send(request);
  if (answer.type !== type) throw new Error(JSON.stringify(answer));
  return answer as Extract<Response, { type: T }>;
}

/** Creates `ada` (full access, admin) with a password. */
export async function createAda(transport: MockTransport, options: { totp?: boolean } = {}) {
  const { user } = await sendOk(
    transport,
    {
      type: "CreateRemoteUser",
      name: "ada",
      display_name: "Ada Lovelace",
      grant: presetGrant("full", { admin: true }),
    },
    "RemoteUserSaved",
  );
  await sendOk(
    transport,
    { type: "SetRemotePassword", user_id: user.id, password: PASSWORD },
    "RemoteUserSaved",
  );
  if (options.totp) {
    await sendOk(transport, { type: "StartTotpEnrollment", user_id: user.id }, "TotpEnrollment");
    await sendOk(
      transport,
      { type: "ConfirmTotpEnrollment", user_id: user.id, code: "123456" },
      "RemoteUserSaved",
    );
  }
  return user;
}

export async function pairingCode(transport: MockTransport, userId: string): Promise<string> {
  return (
    await sendOk(
      transport,
      {
        type: "CreatePairingCode",
        user_id: userId,
        device_name: null,
        grant: null,
        ttl_minutes: null,
      },
      "PairingCodeCreated",
    )
  ).code;
}

/** A browser paired as `name` (logged in). */
export async function pairedBrowser(
  transport: MockTransport,
  userId: string,
  name = "Phone",
  ip = "192.168.1.42",
): Promise<{ browser: MockAuthServer; session: RemoteSession }> {
  const browser = new MockAuthServer(transport, { ip });
  const answer = await browser.send({
    type: "Pair",
    code: await pairingCode(transport, userId),
    device_name: name,
  });
  if (answer.type !== "LoggedIn") throw new Error(JSON.stringify(answer));
  return { browser, session: answer.session };
}

/** A fake WebAuthn: answers with `credentialId`, or throws `error`. */
export function fakePasskeys(
  options: { supported?: boolean; credentialId?: string; error?: unknown } = {},
): PasskeyApi & { calls: unknown[] } {
  const calls: unknown[] = [];
  const answer = (opts: unknown) => {
    calls.push(opts);
    return options.error !== undefined
      ? Promise.reject(options.error)
      : Promise.resolve({ id: options.credentialId ?? "cred-1", type: "public-key" });
  };
  return {
    calls,
    supported: () => options.supported ?? true,
    authenticate: answer,
    register: answer,
  };
}
