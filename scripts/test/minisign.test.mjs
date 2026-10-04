// Tests of scripts/lib/minisign.mjs. Run: node --test scripts/test/
import assert from "node:assert/strict";
import { generateKeyPairSync } from "node:crypto";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import {
  parsePublicKey,
  parseSignature,
  testKey,
  trustedFields,
  unwrapTauriText,
  verifySignature,
  wrapTauriText,
} from "../lib/minisign.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const fixtures = path.join(here, "fixtures", "updater");
const repoRoot = path.resolve(here, "..", "..");

// A signature made by `tauri signer sign --app-version 0.1.1` with the
// development key whose public half is packaging/updater/dev.pub.
const devPub = readFileSync(path.join(repoRoot, "packaging", "updater", "dev.pub"), "utf8");
const fixture = readFileSync(path.join(fixtures, "fixture.txt"));
const fixtureSig = readFileSync(path.join(fixtures, "fixture.txt.sig"), "utf8");

test("a signature made by the Tauri CLI verifies against the committed dev key", () => {
  const fields = verifySignature({ data: fixture, signature: fixtureSig, publicKey: devPub });
  assert.equal(fields.file, "fixture.txt");
  assert.equal(fields.version, "0.1.1");
  assert.match(fields.timestamp ?? "", /^\d+$/);
  // requireSignedVersion: the same version (with or without "v") passes.
  verifySignature({ data: fixture, signature: fixtureSig, publicKey: devPub, version: "v0.1.1" });
});

test("tampered data, another version or another key are refused", () => {
  const changed = Buffer.from(fixture);
  changed.writeUInt8(changed.readUInt8(0) ^ 1, 0);
  assert.throws(
    () => verifySignature({ data: changed, signature: fixtureSig, publicKey: devPub }),
    /does not match the file/,
  );
  assert.throws(
    () =>
      verifySignature({
        data: fixture,
        signature: fixtureSig,
        publicKey: devPub,
        version: "0.2.0",
      }),
    /for version 0\.1\.1, not 0\.2\.0/,
  );
  const other = testKey(generateKeyPairSync("ed25519"), Buffer.from("1111111111111111", "hex"));
  assert.throws(
    () => verifySignature({ data: fixture, signature: fixtureSig, publicKey: other.publicKey }),
    /another key/,
  );
});

test("a changed trusted comment is refused", () => {
  const text = unwrapTauriText(fixtureSig).replace("version:0.1.1", "version:9.9.9");
  assert.throws(
    () => verifySignature({ data: fixture, signature: wrapTauriText(text), publicKey: devPub }),
    /trusted comment was changed/,
  );
});

test("the same key id with another key fails on the signature, not the id", () => {
  const { keyId } = parsePublicKey(devPub);
  const impostor = testKey(generateKeyPairSync("ed25519"), keyId);
  assert.throws(
    () => verifySignature({ data: fixture, signature: fixtureSig, publicKey: impostor.publicKey }),
    /does not match the file/,
  );
});

test("test keys sign like the Tauri CLI", () => {
  const key = testKey(generateKeyPairSync("ed25519"));
  const data = Buffer.from("installer bytes");
  const signature = key.sign(data, {
    file: "Nail_0.2.0_x64-setup.exe",
    version: "0.2.0",
    timestamp: 5,
  });
  assert.deepEqual(
    verifySignature({ data, signature, publicKey: key.publicKey, version: "0.2.0" }),
    {
      timestamp: "5",
      file: "Nail_0.2.0_x64-setup.exe",
      version: "0.2.0",
    },
  );
  const unversioned = key.sign(data);
  verifySignature({ data, signature: unversioned, publicKey: key.publicKey });
  assert.throws(
    () =>
      verifySignature({ data, signature: unversioned, publicKey: key.publicKey, version: "0.2.0" }),
    /carries no version/,
  );
  assert.equal(parseSignature(signature).algorithm, "ED");
});

test("parsing accepts Tauri's base64 form and the plain text, and refuses junk", () => {
  const plain = unwrapTauriText(devPub);
  assert.match(plain, /^untrusted comment: minisign public key: 39C9F75999FD4B2C\n/);
  assert.deepEqual(parsePublicKey(plain), parsePublicKey(devPub));
  assert.equal(parsePublicKey(devPub).publicKey.length, 32);
  assert.throws(() => unwrapTauriText("aGVsbG8="), /not a minisign/);
  assert.throws(() => parsePublicKey(wrapTauriText("untrusted comment: x\nAAAA\n")), /unsupported/);
  assert.throws(() => parseSignature(wrapTauriText("untrusted comment: x\n")), /malformed/);
  assert.throws(
    () => parseSignature(wrapTauriText("untrusted comment: x\nAAAA\nnot trusted\nAAAA\n")),
    /trusted comment/,
  );
});

test("trusted comment fields", () => {
  assert.deepEqual(trustedFields("timestamp:1\tfile:a:b.exe\tversion:1.0.0\tjunk"), {
    timestamp: "1",
    file: "a:b.exe",
    version: "1.0.0",
  });
});
