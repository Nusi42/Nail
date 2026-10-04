// Tests of scripts/lib/update-server.mjs and scripts/lib/update-check.mjs:
// the loopback update server serving a manifest and an installer, and the
// updater's check / verified download against it. Binds 127.0.0.1 only.
import assert from "node:assert/strict";
import { generateKeyPairSync } from "node:crypto";
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { after, before, test } from "node:test";

import { testKey } from "../lib/minisign.mjs";
import { checkEndpoint, checkForUpdate, downloadVerified } from "../lib/update-check.mjs";
import { createUpdateServer, listenLoopback, resolveUnder } from "../lib/update-server.mjs";
import { buildManifest, collectPlatforms, manifestJson } from "../lib/update-manifest.mjs";

const key = testKey(generateKeyPairSync("ed25519"));
const installer = Buffer.from("pretend this is Nail_0.2.0_x64-setup.exe");
let root = "";
/** @type {import("node:http").Server | undefined} */
let server;
let base = "";
/** @type {import("../lib/update-server.mjs").LogEntry[]} */
const requests = [];

before(async () => {
  root = mkdtempSync(path.join(tmpdir(), "nail-update-server-"));
  server = createUpdateServer({
    root,
    log: (e) => {
      requests.push(e);
    },
  });
  const port = await listenLoopback(server, 0);
  base = `http://127.0.0.1:${String(port)}`;
  writeFileSync(path.join(root, "Nail_0.2.0_x64-setup.exe"), installer);
  writeFileSync(path.join(root, "Nail_0.2.0_x64_en-US.msi"), Buffer.from("msi"));
  const files = [
    {
      name: "Nail_0.2.0_x64-setup.exe",
      signature: key.sign(installer, { file: "Nail_0.2.0_x64-setup.exe", version: "0.2.0" }),
    },
    {
      name: "Nail_0.2.0_x64_en-US.msi",
      // Signed for another version: the msi update must be refused.
      signature: key.sign(Buffer.from("msi"), {
        file: "Nail_0.2.0_x64_en-US.msi",
        version: "0.1.9",
      }),
    },
  ];
  const entries = collectPlatforms({ files, version: "0.2.0", downloadBase: base });
  const manifest = buildManifest({
    version: "0.2.0",
    channel: "stable",
    notes: "notes",
    pubDate: "2026-10-02T00:00:00Z",
    entries,
  });
  mkdirSync(path.join(root, "stable"));
  writeFileSync(path.join(root, "stable", "latest.json"), manifestJson(manifest));
});

after(() => {
  server?.close();
  rmSync(root, { recursive: true, force: true });
});

test("paths stay inside the root", () => {
  const r = path.resolve("srv");
  assert.equal(resolveUnder(r, "/stable/latest.json?x=1"), path.join(r, "stable", "latest.json"));
  assert.equal(resolveUnder(r, "/a%20b.exe"), path.join(r, "a b.exe"));
  for (const bad of ["/", "/../x", "/a/../../x", "/%2e%2e/x", "/a%5cb", "/%00", "/%zz"]) {
    assert.equal(resolveUnder(r, bad), null, bad);
  }
});

test("the server serves files, refuses the rest, and logs every request", async () => {
  const res = await fetch(`${base}/stable/latest.json`);
  assert.equal(res.status, 200);
  assert.equal(res.headers.get("content-type"), "application/json");
  assert.equal(/** @type {{ version?: string }} */ (await res.json()).version, "0.2.0");
  assert.equal((await fetch(`${base}/missing.json`)).status, 404);
  assert.equal((await fetch(`${base}/stable`)).status, 404);
  assert.equal((await fetch(`${base}/stable/latest.json`, { method: "POST" })).status, 405);
  const head = await fetch(`${base}/Nail_0.2.0_x64-setup.exe`, { method: "HEAD" });
  assert.equal(head.status, 200);
  assert.equal(head.headers.get("content-length"), String(installer.length));
  assert.ok(requests.some((r) => r.path === "/missing.json" && r.status === 404));
  assert.ok(requests.some((r) => r.method === "POST" && r.status === 405));
});

test("the updater finds the newer NSIS build and downloads it verified", async () => {
  const update = await checkForUpdate({
    endpoint: `${base}/stable/latest.json`,
    currentVersion: "0.1.0",
    arch: "x64",
    installer: "nsis",
  });
  assert.equal(update.available, true);
  assert.equal(update.version, "0.2.0");
  assert.equal(update.target, "windows-x86_64-nsis");
  const data = await downloadVerified({ update, publicKey: key.publicKey });
  assert.deepEqual(data, installer);
  // Another key's public half refuses the same download.
  const other = testKey(generateKeyPairSync("ed25519"), Buffer.from("2222222222222222", "hex"));
  await assert.rejects(downloadVerified({ update, publicKey: other.publicKey }), /another key/);
});

test("no update for the same or a newer version, or a missing manifest", async () => {
  for (const currentVersion of ["0.2.0", "0.3.0"]) {
    const update = await checkForUpdate({
      endpoint: `${base}/stable/latest.json`,
      currentVersion,
      arch: "x64",
      installer: "nsis",
    });
    assert.equal(update.available, false);
  }
  const none = await checkForUpdate({
    endpoint: `${base}/beta/latest.json`,
    currentVersion: "0.1.0",
    arch: "x64",
    installer: "nsis",
  });
  assert.equal(none.available, false);
});

test("an MSI update signed for another version is refused; unknown targets fail", async () => {
  const msi = await checkForUpdate({
    endpoint: `${base}/stable/latest.json`,
    currentVersion: "0.1.0",
    arch: "x64",
    installer: "msi",
  });
  assert.equal(msi.target, "windows-x86_64-msi");
  await assert.rejects(
    downloadVerified({ update: msi, publicKey: key.publicKey }),
    /version 0\.1\.9/,
  );
  await assert.rejects(
    checkForUpdate({
      endpoint: `${base}/stable/latest.json`,
      currentVersion: "0.1.0",
      arch: "arm64",
      installer: "nsis",
    }),
    /no entry for windows-aarch64-nsis/,
  );
});

test("endpoints: https, or http on loopback only", () => {
  assert.equal(checkEndpoint("https://u.example/stable/latest.json").hostname, "u.example");
  assert.equal(checkEndpoint("http://127.0.0.1:1/x").port, "1");
  assert.throws(() => checkEndpoint("http://u.example/x"), /https/);
  assert.throws(() => checkEndpoint("file:///c:/x"), /https/);
});
