/**
 * Helpers for the remote-UI scenarios (M2b, ADR 0041): requests to the
 * core over the local server's WebSocket (what the desktop UI would send),
 * turning the remote UI on, RFC 6238 codes for two-step verification, and
 * a WebAuthn virtual authenticator.
 */
import { createHash, createHmac } from "node:crypto";
import { connect } from "node:tls";
import { chromium, type Browser, type BrowserContext, type Page } from "@playwright/test";
import { freePort, type NailServer } from "./nail";

/** A core answer (`nail_api::Response`), loosely typed. */
export type CoreResponse = { type: string } & Record<string, unknown>;

/** Sends one request to the core over the local server's WebSocket. */
export async function rpc(server: NailServer, request: object): Promise<CoreResponse> {
  const url = new URL("api/ws", server.url);
  url.protocol = url.protocol.replace("http", "ws");
  const socket = new WebSocket(url);
  return new Promise<CoreResponse>((resolve, reject) => {
    const timer = setTimeout(() => {
      socket.close();
      reject(new Error(`no answer to ${JSON.stringify(request)}`));
    }, 30_000);
    socket.addEventListener("open", () => {
      socket.send(JSON.stringify({ id: 1, request }));
    });
    socket.addEventListener("message", (message) => {
      const frame = JSON.parse(String(message.data)) as {
        kind: string;
        id?: number;
        response?: CoreResponse;
      };
      if (frame.kind === "response" && frame.id === 1 && frame.response) {
        clearTimeout(timer);
        socket.close();
        resolve(frame.response);
      }
    });
    socket.addEventListener("error", () => {
      clearTimeout(timer);
      reject(new Error("WebSocket error"));
    });
  });
}

/** Like {@link rpc}, failing unless the answer has type `expected`. */
export async function rpcOk(
  server: NailServer,
  request: object,
  expected: string,
): Promise<CoreResponse> {
  const answer = await rpc(server, request);
  if (answer.type !== expected) {
    throw new Error(`${JSON.stringify(request)} answered ${JSON.stringify(answer)}`);
  }
  return answer;
}

/** The full-access grant (all accounts), optionally with `admin`. */
export function fullGrant(admin = false): object {
  return {
    mail: {
      scopes: ["read", "raw-source", "attachments", "organize", "draft", "send"],
      all_accounts: true,
      accounts: [],
      excluded_folders: [],
      skip_draft_confirmation: false,
    },
    admin,
  };
}

/** A remote user; returns its id. */
export async function createUser(server: NailServer, name: string, admin = false): Promise<string> {
  const saved = await rpcOk(
    server,
    { type: "CreateRemoteUser", name, display_name: null, grant: fullGrant(admin) },
    "RemoteUserSaved",
  );
  return (saved.user as { id: string }).id;
}

/** A pairing code for `userId`. */
export async function pairingCode(server: NailServer, userId: string): Promise<string> {
  const created = await rpcOk(
    server,
    { type: "CreatePairingCode", user_id: userId, device_name: null, grant: null, ttl_minutes: 10 },
    "PairingCodeCreated",
  );
  return created.code as string;
}

export interface RemoteUi {
  /** `https://localhost:<port>/` */
  url: string;
  port: number;
}

/**
 * Turns the remote UI on (TLS with a self-signed certificate, host name
 * `localhost` so passkeys work) and waits until it runs.
 */
export async function enableRemoteUi(server: NailServer): Promise<RemoteUi> {
  const port = freePort();
  await rpcOk(
    server,
    {
      type: "UpdateRemoteUiSettings",
      settings: {
        enabled: true,
        bind_address: "127.0.0.1",
        port,
        host_name: "localhost",
        tls: "self-signed",
        acme: { domains: [], contact_email: null, directory_url: "", accept_terms: false },
        internet_mode: false,
        allowed_networks: [],
        session_idle_minutes: 30,
        session_max_hours: 12,
        reauth_minutes: 5,
      },
    },
    "RemoteUi",
  );
  for (let i = 0; i < 200; i++) {
    const answer = await rpcOk(server, { type: "GetRemoteUiSettings" }, "RemoteUi");
    const status = answer.status as { state: string; urls: string[]; message: string | null };
    if (status.state === "running") return { url: status.urls[0] ?? "", port };
    if (status.state === "failed")
      throw new Error(`remote UI failed: ${status.message ?? "(no message)"}`);
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error("the remote UI did not start");
}

/** A new browser context for the remote UI (self-signed certificate). */
export function remoteContext(page: Page): Promise<BrowserContext> {
  const browser = page.context().browser();
  if (!browser) throw new Error("no browser");
  return browser.newContext({ ignoreHTTPSErrors: true });
}

/**
 * A browser that trusts the remote UI's self-signed certificate the way a
 * paired device does (by its key): Chrome's SPKI allow-list. Unlike
 * `ignoreHTTPSErrors`, the page then has no certificate error, which
 * WebAuthn requires.
 */
export async function trustingBrowser(remote: RemoteUi): Promise<Browser> {
  const spki = await new Promise<string>((resolve, reject) => {
    const socket = connect(
      { host: "127.0.0.1", port: remote.port, servername: "localhost", rejectUnauthorized: false },
      () => {
        const der = socket.getPeerX509Certificate()?.publicKey.export({
          type: "spki",
          format: "der",
        });
        socket.end();
        if (der) resolve(createHash("sha256").update(der).digest("base64"));
        else reject(new Error("no certificate"));
      },
    );
    socket.once("error", reject);
  });
  return chromium.launch({ args: [`--ignore-certificate-errors-spki-list=${spki}`] });
}

/** RFC 6238 code (SHA-1, 6 digits, 30 s) for a base32 secret at `unixSeconds`. */
export function totp(secretBase32: string, unixSeconds = Date.now() / 1000): string {
  const alphabet = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
  let bits = "";
  for (const char of secretBase32.replace(/=+$/, "").toUpperCase()) {
    const value = alphabet.indexOf(char);
    if (value < 0) continue;
    bits += value.toString(2).padStart(5, "0");
  }
  const bytes: number[] = [];
  for (let i = 0; i + 8 <= bits.length; i += 8) bytes.push(parseInt(bits.slice(i, i + 8), 2));
  const counter = Math.floor(unixSeconds / 30);
  const message = Buffer.alloc(8);
  message.writeBigUInt64BE(BigInt(counter));
  const hmac = createHmac("sha1", Buffer.from(bytes)).update(message).digest();
  const offset = (hmac[hmac.length - 1] ?? 0) & 0x0f;
  const code =
    (((hmac[offset] ?? 0) & 0x7f) << 24) |
    ((hmac[offset + 1] ?? 0) << 16) |
    ((hmac[offset + 2] ?? 0) << 8) |
    (hmac[offset + 3] ?? 0);
  return String(code % 1_000_000).padStart(6, "0");
}

/** Adds a CTAP2 platform authenticator with user verification to `page`. */
export async function virtualAuthenticator(page: Page): Promise<void> {
  const cdp = await page.context().newCDPSession(page);
  await cdp.send("WebAuthn.enable");
  await cdp.send("WebAuthn.addVirtualAuthenticator", {
    options: {
      protocol: "ctap2",
      transport: "internal",
      hasResidentKey: true,
      hasUserVerification: true,
      isUserVerified: true,
      automaticPresenceSimulation: true,
    },
  });
}

/** Pairs the browser on `page` (at the remote UI) with `code`. */
export async function pairBrowser(page: Page, remote: RemoteUi, code: string, device = "Phone") {
  await page.goto(remote.url);
  const form = page.getByRole("form", { name: "Pair this browser" });
  await form.getByRole("textbox", { name: "Pairing code" }).fill(code);
  await form.getByRole("textbox", { name: "Name for this device" }).fill(device);
  await form.getByRole("button", { name: "Pair this browser" }).click();
}
