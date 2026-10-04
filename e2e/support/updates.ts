/**
 * A loopback update server for the desktop updater scenario (M4b WP115):
 * a throwaway minisign key in the form the Tauri updater reads (same
 * format as `scripts/lib/minisign.mjs` / `tauri signer`), a signed
 * "installer" and `<channel>/latest.json` manifests, served on 127.0.0.1
 * only (ADR 0057).
 */
import { createHash, createPrivateKey, generateKeyPairSync, sign } from "node:crypto";
import { readFileSync } from "node:fs";
import { createServer, type Server } from "node:http";
import type { AddressInfo } from "node:net";
import { defined } from "./defined";

const KEY_ID = Buffer.from("4e41494c45324531", "hex");

function wrap(text: string): string {
  return Buffer.from(text, "utf8").toString("base64");
}

/** A key pair: the public key as the app takes it, and a signer. */
export function updateKey() {
  const pair = generateKeyPairSync("ed25519");
  const jwk = pair.privateKey.export({ format: "jwk" });
  const raw = Buffer.from(defined(jwk.x, "the key's x"), "base64url");
  const privateKey = createPrivateKey({ key: jwk, format: "jwk" });
  const publicKey = wrap(
    `untrusted comment: minisign public key: ${KEY_ID.toString("hex").toUpperCase()}\n` +
      `${Buffer.concat([Buffer.from("Ed", "latin1"), KEY_ID, raw]).toString("base64")}\n`,
  );
  return {
    publicKey,
    /** Signs like `tauri signer sign` (BLAKE2b-prehashed, version in the trusted comment). */
    sign(data: Buffer, file: string, version: string): string {
      const digest = createHash("blake2b512").update(data).digest();
      const signature = sign(null, digest, privateKey);
      const trusted = `timestamp:${String(Math.floor(Date.now() / 1000))}\tfile:${file}\tversion:${version}`;
      const global = sign(
        null,
        Buffer.concat([signature, Buffer.from(trusted, "utf8")]),
        privateKey,
      );
      return wrap(
        "untrusted comment: signature from tauri secret key\n" +
          `${Buffer.concat([Buffer.from("ED", "latin1"), KEY_ID, signature]).toString("base64")}\n` +
          `trusted comment: ${trusted}\n` +
          `${global.toString("base64")}\n`,
      );
    },
  };
}

export interface UpdateServer {
  /** `http://127.0.0.1:<port>` */
  base: string;
  /** The endpoint the app is given (`{{channel}}` is the shell's placeholder). */
  endpoint: string;
  /** Paths requested so far. */
  requests: string[];
  close(): Promise<void>;
}

/**
 * Serves `installer` as version `version` on the stable channel (beta has
 * no newer version), signed with `key`.
 */
export async function startUpdateServer(options: {
  installer: string;
  version: string;
  notes: string;
  key: ReturnType<typeof updateKey>;
}): Promise<UpdateServer> {
  const bytes = readFileSync(options.installer);
  const file = `Nail_${options.version}_x64-setup.exe`;
  const signature = options.key.sign(bytes, file, options.version);
  const requests: string[] = [];
  let base = "";
  const manifest = () => {
    const target = { signature, url: `${base}/files/${file}` };
    return JSON.stringify({
      version: options.version,
      notes: options.notes,
      pub_date: "2026-10-01T12:00:00Z",
      platforms: { "windows-x86_64-nsis": target, "windows-x86_64": target },
    });
  };
  const server: Server = createServer((req, res) => {
    const url = req.url ?? "/";
    requests.push(url);
    if (url === "/stable/latest.json") {
      res.writeHead(200, { "content-type": "application/json" }).end(manifest());
    } else if (url === `/files/${file}`) {
      res
        .writeHead(200, {
          "content-type": "application/octet-stream",
          "content-length": String(bytes.length),
        })
        .end(bytes);
    } else {
      // No beta release, nothing else.
      res.writeHead(404).end();
    }
  });
  await new Promise<void>((resolve) => server.listen(0, "127.0.0.1", resolve));
  base = `http://127.0.0.1:${String((server.address() as AddressInfo).port)}`;
  return {
    base,
    endpoint: `${base}/{{channel}}/latest.json`,
    requests,
    close: () =>
      new Promise((resolve) =>
        server.close(() => {
          resolve();
        }),
      ),
  };
}
