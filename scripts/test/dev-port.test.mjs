// Tests of scripts/lib/dev-port.mjs (no fixed development ports). Run: node --test scripts/test/
import assert from "node:assert/strict";
import fs from "node:fs";
import net from "node:net";
import os from "node:os";
import path from "node:path";
import { test } from "node:test";

import {
  DEV_PORT_ENV,
  PREFERRED_DEV_PORT,
  devUrl,
  freePort,
  isPortFree,
  parseDevArgs,
  parsePortEnv,
  pickDevPort,
  tauriDevConfig,
  writeTauriDevConfig,
} from "../lib/dev-port.mjs";
import { readJson } from "../lib/json.mjs";

/**
 * Holds a free 127.0.0.1 port for the duration of a test.
 *
 * @returns {Promise<{ port: number, release: () => Promise<void> }>}
 */
function hold() {
  return new Promise((resolve, reject) => {
    const server = net.createServer();
    server.once("error", reject);
    server.listen({ port: 0, host: "127.0.0.1" }, () => {
      const address = server.address();
      if (address === null || typeof address === "string") {
        reject(new Error("not a TCP server"));
        return;
      }
      resolve({
        port: address.port,
        release: () =>
          new Promise((done) => {
            server.close(() => {
              done();
            });
          }),
      });
    });
  });
}

test("the preferred port is not Vite's default", () => {
  assert.notEqual(PREFERRED_DEV_PORT, 5173);
});

test("NAIL_DEV_PORT values are checked", () => {
  assert.equal(parsePortEnv(undefined), undefined);
  assert.equal(parsePortEnv(""), undefined);
  assert.equal(parsePortEnv(" 6123 "), 6123);
  for (const bad of ["0", "65536", "abc", "12.5", "-3"]) {
    assert.throws(() => parsePortEnv(bad), /NAIL_DEV_PORT/);
  }
});

test("a held port is not free, a released one is", async () => {
  const { port, release } = await hold();
  assert.equal(await isPortFree(port), false);
  await release();
  assert.equal(await isPortFree(port), true);
  const any = await freePort();
  assert.ok(any > 0 && any <= 65535);
});

test("the preferred port is used when free", async () => {
  const preferred = await freePort();
  assert.deepEqual(await pickDevPort({ env: {}, preferred }), {
    port: preferred,
    source: "preferred",
  });
});

test("a taken preferred port falls back to a free one", async () => {
  const { port: taken, release } = await hold();
  try {
    const picked = await pickDevPort({ env: {}, preferred: taken });
    assert.equal(picked.source, "free");
    assert.notEqual(picked.port, taken);
    assert.equal(await isPortFree(picked.port), true);
  } finally {
    await release();
  }
});

test("NAIL_DEV_PORT is used exactly and a taken one is an error", async () => {
  const wanted = await freePort();
  assert.deepEqual(await pickDevPort({ env: { [DEV_PORT_ENV]: String(wanted) }, preferred: 1 }), {
    port: wanted,
    source: "env",
  });
  const { port: taken, release } = await hold();
  try {
    await assert.rejects(pickDevPort({ env: { [DEV_PORT_ENV]: String(taken) } }), /is taken/);
  } finally {
    await release();
  }
});

test("the Tauri override points devUrl at the port and drops beforeDevCommand", () => {
  assert.equal(devUrl(6123), "http://127.0.0.1:6123");
  assert.deepEqual(tauriDevConfig(6123), {
    build: { devUrl: "http://127.0.0.1:6123", beforeDevCommand: null },
  });
  const dir = fs.mkdtempSync(path.join(os.tmpdir(), "nail-dev-port-"));
  try {
    const file = writeTauriDevConfig(6123, dir);
    assert.deepEqual(readJson(file), tauriDevConfig(6123));
  } finally {
    fs.rmSync(dir, { recursive: true, force: true });
  }
});

test("`tauri` as the first argument runs the desktop app with the rest", () => {
  assert.deepEqual(parseDevArgs([]), { tauri: false, tauriArgs: [] });
  assert.deepEqual(parseDevArgs(["tauri", "--release", "--", "--seed", "demo"]), {
    tauri: true,
    tauriArgs: ["--release", "--", "--seed", "demo"],
  });
});

test("tauri.conf.json has no fixed Vite port and no dev command of its own", () => {
  const conf = /** @type {{ build: { beforeDevCommand?: string, devUrl?: string } }} */ (
    readJson(new URL("../../app/src-tauri/tauri.conf.json", import.meta.url))
  );
  assert.equal(conf.build.beforeDevCommand, undefined);
  assert.equal(conf.build.devUrl, devUrl(PREFERRED_DEV_PORT));
});

test("vite.config.ts takes its port from NAIL_DEV_PORT, not a fixed 5173", () => {
  const text = fs.readFileSync(new URL("../../ui/vite.config.ts", import.meta.url), "utf8");
  assert.doesNotMatch(text, /5173/);
  assert.match(text, /NAIL_DEV_PORT/);
});
