// Tests of scripts/lib/sign.mjs. Run: node --test scripts/test/
import assert from "node:assert/strict";
import path from "node:path";
import { test } from "node:test";

import {
  DEFAULT_TIMESTAMP_URL,
  findSigntool,
  signingFromEnv,
  signtoolArgs,
  tauriSigning,
  verifyArgs,
} from "../lib/sign.mjs";

const thumb = "0123456789abcdef0123456789ABCDEF01234567";

test("no certificate configured: nothing is signed", () => {
  assert.equal(signingFromEnv({}), null);
  assert.equal(signingFromEnv({ NAIL_SIGN_THUMBPRINT: "" }), null);
  assert.equal(signingFromEnv({ NAIL_SIGN_THUMBPRINT: "  " }), null);
  assert.equal(tauriSigning(null), null);
});

test("a configured certificate: thumbprint normalized, defaults filled in", () => {
  const signing = signingFromEnv({ NAIL_SIGN_THUMBPRINT: "01:23 45" + thumb.slice(6) });
  assert.ok(signing);
  assert.equal(signing.thumbprint, thumb.toUpperCase());
  assert.equal(signing.timestampUrl, DEFAULT_TIMESTAMP_URL);
  assert.equal(signing.description, "Nail");
  assert.equal(signing.signtool, null);
  const custom = signingFromEnv({
    NAIL_SIGN_THUMBPRINT: thumb,
    NAIL_SIGN_TIMESTAMP_URL: "https://ts.example/rfc3161",
    NAIL_SIGN_DESCRIPTION: "Nail mail",
    NAIL_SIGNTOOL: "C:\\sdk\\signtool.exe",
  });
  assert.ok(custom);
  assert.equal(custom.timestampUrl, "https://ts.example/rfc3161");
  assert.equal(custom.signtool, "C:\\sdk\\signtool.exe");
  assert.throws(() => signingFromEnv({ NAIL_SIGN_THUMBPRINT: "abc" }), /40-digit/);
  assert.throws(
    () => signingFromEnv({ NAIL_SIGN_THUMBPRINT: thumb, NAIL_SIGN_TIMESTAMP_URL: "ts" }),
    /must be a URL/,
  );
});

test("signtool and bundler settings: SHA-256 with an RFC 3161 timestamp", () => {
  const signing = signingFromEnv({ NAIL_SIGN_THUMBPRINT: thumb });
  assert.ok(signing);
  assert.deepEqual(signtoolArgs(signing, "C:\\b\\nail.exe"), [
    "sign",
    "/sha1",
    thumb.toUpperCase(),
    "/fd",
    "sha256",
    "/tr",
    DEFAULT_TIMESTAMP_URL,
    "/td",
    "sha256",
    "/d",
    "Nail",
    "C:\\b\\nail.exe",
  ]);
  assert.deepEqual(verifyArgs("x.exe"), ["verify", "/pa", "/all", "x.exe"]);
  assert.deepEqual(tauriSigning(signing), {
    windows: {
      certificateThumbprint: thumb.toUpperCase(),
      digestAlgorithm: "sha256",
      timestampUrl: DEFAULT_TIMESTAMP_URL,
      tsp: true,
    },
  });
});

test("signtool is found in the newest Windows SDK", () => {
  const bin = path.join("PF86", "Windows Kits", "10", "bin");
  const present = new Set([
    path.join(bin, "10.0.26100.0", "x64", "signtool.exe"),
    path.join(bin, "10.0.9600.0", "x64", "signtool.exe"),
  ]);
  const found = findSigntool({
    signing: { signtool: null },
    programFilesX86: "PF86",
    readdir: () => ["10.0.9600.0", "arm64", "10.0.26100.0", "10.0.30000.0"],
    exists: (f) => present.has(f),
  });
  assert.equal(found, path.join(bin, "10.0.26100.0", "x64", "signtool.exe"));
  assert.equal(
    findSigntool({ signing: { signtool: "mine.exe" }, readdir: () => [], exists: () => false }),
    "mine.exe",
  );
  assert.throws(
    () =>
      findSigntool({
        signing: null,
        programFilesX86: "PF86",
        readdir: () => {
          throw new Error("ENOENT");
        },
        exists: () => false,
      }),
    /signtool\.exe not found/,
  );
});
