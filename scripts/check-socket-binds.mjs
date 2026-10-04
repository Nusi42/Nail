#!/usr/bin/env node
/**
 * Fails when crates/, app/ or e2e/ bind a socket to an unspecified or
 * non-loopback address outside the allowlist (ADR 0057; rules and
 * allowlist in scripts/lib/socket-binds.mjs). Run by `npm test` and CI.
 *
 *   node scripts/check-socket-binds.mjs
 */
import path from "node:path";
import { fileURLToPath } from "node:url";

import { checkRepository } from "./lib/socket-binds.mjs";
import { assertWorkFolder } from "./lib/work-folder.mjs";

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

assertWorkFolder("npm test");
const { files, violations, stale } = checkRepository(root);

for (const v of violations) {
  console.error(`${v.file}:${String(v.line)}: ${v.kind} (${v.name}): ${v.text}`);
}
for (const s of stale) {
  console.error(`stale allowlist entry: ${s.file}: ${s.text}`);
}
if (violations.length > 0 || stale.length > 0) {
  console.error(
    "\nNail never opens sockets on non-loopback or unspecified addresses (ADR 0057).\n" +
      "Bind 127.0.0.1 / ::1 instead, or add a reasoned entry to ALLOWLIST in scripts/lib/socket-binds.mjs.",
  );
  process.exit(1);
}
console.log(`socket binds: ${String(files)} files checked, loopback only`);
