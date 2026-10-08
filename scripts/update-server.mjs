#!/usr/bin/env node
/**
 * Loopback update server (plan M4b WP112): serves a folder of update
 * manifests (`<channel>/latest.json`) and installers on 127.0.0.1 until
 * stopped.
 *
 *   node scripts/update-server.mjs --root <dir> [--port 47811] [--port-file <file>] [--log <file>]
 *
 * --port-file receives the port once listening (so callers can wait for
 * it); --log gets one JSON line per request.
 */
import { appendFileSync, writeFileSync } from "node:fs";

import { createUpdateServer, listenLoopback } from "./lib/update-server.mjs";
import { forEachArg } from "./lib/args.mjs";

/** @param {string[]} argv */
function parse(argv) {
  /** @type {{ port: number, root?: string, portFile?: string, log?: string }} */
  const o = { port: 0 };
  forEachArg(argv, (arg, { value }) => {
    if (arg === "--root") o.root = value();
    else if (arg === "--port") o.port = Number(value());
    else if (arg === "--port-file") o.portFile = value();
    else if (arg === "--log") o.log = value();
    else throw new Error(`unknown option ${arg}`);
  });
  const { root } = o;
  if (!root) throw new Error("--root is required");
  if (!Number.isInteger(o.port) || o.port < 0 || o.port > 65535) throw new Error("bad --port");
  return { ...o, root };
}

const o = parse(process.argv.slice(2));
const server = createUpdateServer({
  root: o.root,
  log: (entry) => {
    const line = JSON.stringify({ time: new Date().toISOString(), ...entry });
    console.log(line);
    if (o.log) appendFileSync(o.log, `${line}\n`);
  },
});
const port = await listenLoopback(server, o.port);
if (o.portFile) writeFileSync(o.portFile, String(port));
console.log(`update server: http://127.0.0.1:${String(port)}/ (root ${o.root})`);
