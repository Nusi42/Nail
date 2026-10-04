/**
 * A static file server for update manifests and installers, bound to
 * 127.0.0.1 only (ADR 0057: no non-loopback sockets) — the "loopback update
 * server" of the installer smoke test (plan M4b WP112) and of the desktop
 * update scenario (WP115). GET / HEAD only, no directory listings, nothing
 * outside the root; every request is reported to `log`.
 * CLI: scripts/update-server.mjs; tests: scripts/test/update-server.test.mjs.
 */
import { createReadStream, statSync } from "node:fs";
import { createServer } from "node:http";
import path from "node:path";

/** @type {Partial<Record<string, string>>} */
const TYPES = {
  ".json": "application/json",
  ".exe": "application/vnd.microsoft.portable-executable",
  ".msi": "application/x-msi",
  ".sig": "text/plain; charset=utf-8",
  ".zip": "application/zip",
};

/**
 * The file a URL path names below `root`, or null (traversal, odd paths).
 *
 * @param {string} root
 * @param {string} urlPath
 * @returns {string | null}
 */
export function resolveUnder(root, urlPath) {
  let decoded;
  try {
    decoded = decodeURIComponent(urlPath.split("?")[0] ?? "");
  } catch {
    return null;
  }
  if (decoded.includes("\0") || decoded.includes("\\")) return null;
  const parts = decoded.split("/").filter(Boolean);
  if (parts.some((p) => p === "." || p === "..")) return null;
  const base = path.resolve(root);
  const file = path.resolve(base, ...parts);
  const rel = path.relative(base, file);
  if (!rel || rel.startsWith("..") || path.isAbsolute(rel)) return null;
  return file;
}

/**
 * One served request.
 *
 * @typedef {{ method: string | undefined, path: string | undefined, status: number }} LogEntry
 */

/**
 * The server (not listening yet); `log(entry)` gets {method, path, status}.
 *
 * @param {{ root: string, log?: (entry: LogEntry) => void }} options
 */
export function createUpdateServer({
  root,
  log = () => {
    /* no-op */
  },
}) {
  return createServer((req, res) => {
    /**
     * @param {number} status
     * @param {Record<string, string>} [headers]
     * @param {string | null} [body]
     */
    const finish = (status, headers = {}, body = null) => {
      log({ method: req.method, path: req.url, status });
      res.writeHead(status, headers);
      res.end(body);
    };
    if (req.method !== "GET" && req.method !== "HEAD") {
      finish(405, { allow: "GET, HEAD" });
      return;
    }
    const file = resolveUnder(root, req.url ?? "/");
    let stat;
    try {
      stat = file ? statSync(file) : null;
    } catch {
      stat = null;
    }
    if (!file || !stat?.isFile()) {
      finish(404, { "content-type": "text/plain" }, "not found");
      return;
    }
    const headers = {
      "content-type": TYPES[path.extname(file).toLowerCase()] ?? "application/octet-stream",
      "content-length": String(stat.size),
      "cache-control": "no-store",
    };
    log({ method: req.method, path: req.url, status: 200 });
    res.writeHead(200, headers);
    if (req.method === "HEAD") {
      res.end();
      return;
    }
    createReadStream(file).pipe(res);
  });
}

/**
 * Starts it on 127.0.0.1:`port` (0 = any free port); resolves the port.
 *
 * @param {import("node:http").Server} server
 * @param {number} port
 * @returns {Promise<number>}
 */
export function listenLoopback(server, port) {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", () => {
      const address = server.address();
      if (address === null || typeof address === "string") {
        reject(new Error("the update server is not listening on TCP"));
        return;
      }
      resolve(address.port);
    });
  });
}
