#!/usr/bin/env node
/**
 * The development update key (plan M4b WP112). Usage:
 *
 *   node scripts/updater-key.mjs            check: the local private key signs
 *                                           what packaging/updater/dev.pub verifies
 *   node scripts/updater-key.mjs --generate create the key pair (outside the
 *                                           repository) and copy the public half
 *                                           to packaging/updater/dev.pub
 *
 * The key has an empty password (it signs nothing anyone installs); set
 * NAIL_DEV_UPDATER_KEY_PASSWORD when you made one with a password. The
 * release key is made the same way (`npx tauri signer generate -w <file>`)
 * on an offline machine and stored only as CI secrets (docs/release.md).
 */
import { execFileSync } from "node:child_process";
import { copyFileSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { homedir } from "node:os";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { assertOutsideRepo, keysDir } from "./lib/build.mjs";
import { verifySignature } from "./lib/minisign.mjs";
import { devKeyPaths } from "./lib/updater-key.mjs";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

/**
 * Runs `npx` with `args`; returns its standard output.
 *
 * @param {string[]} args
 * @param {NodeJS.ProcessEnv} [env]
 */
function npx(args, env = process.env) {
  // npx is a .cmd file on Windows: run it through the shell, one string.
  const quoted = args.map((a) =>
    a === "" || /[\s"]/.test(a) ? `"${a.replaceAll('"', '""')}"` : a,
  );
  return execFileSync(["npx", ...quoted].join(" "), {
    cwd: repoRoot,
    encoding: "utf8",
    env,
    shell: true,
  });
}

function main() {
  const generate = process.argv.includes("--generate");
  const keys = keysDir(process.env, homedir());
  const paths = devKeyPaths({ keysDir: keys, repoRoot });
  assertOutsideRepo(paths.privateKey, repoRoot);
  const password = process.env.NAIL_DEV_UPDATER_KEY_PASSWORD ?? "";

  if (generate) {
    if (existsSync(paths.privateKey)) {
      throw new Error(`${paths.privateKey} exists; delete it first to make a new development key`);
    }
    mkdirSync(path.dirname(paths.privateKey), { recursive: true });
    npx(["tauri", "signer", "generate", "--ci", "-p", password, "-w", paths.privateKey]);
    copyFileSync(paths.generatedPublicKey, paths.publicKey);
    console.log(`private key: ${paths.privateKey} (never commit it)`);
    console.log(`public key:  ${paths.publicKey} (commit this one)`);
  }

  if (!existsSync(paths.privateKey)) {
    throw new Error(`no development key at ${paths.privateKey}; run with --generate`);
  }
  // Sign a probe and check it against the committed public key.
  const probe = path.join(keys, "probe.txt");
  writeFileSync(probe, `nail updater key probe ${String(Date.now())}\n`);
  try {
    npx([
      "tauri",
      "signer",
      "sign",
      "-f",
      paths.privateKey,
      "-p",
      password,
      "--app-version",
      "0.0.0",
      probe,
    ]);
    verifySignature({
      data: readFileSync(probe),
      signature: readFileSync(`${probe}.sig`, "utf8"),
      publicKey: readFileSync(paths.publicKey, "utf8"),
      version: "0.0.0",
    });
  } finally {
    rmSync(probe, { force: true });
    rmSync(`${probe}.sig`, { force: true });
  }
  console.log("development update key: ok (matches packaging/updater/dev.pub)");
}

main();
