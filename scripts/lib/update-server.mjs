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

const TYPES = {
  ".json": "application/json",
  ".exe": "application/vnd.microsoft.portable-executable",
  ".msi": "application/x-msi",
  ".sig": "text/plain; charset=utf-8",
  ".zip": "application/zip",
};

/** The file a URL path names below `root`, or null (traversal, odd paths). */
export function resolveUnder(root, urlPath) {
  let decoded;
  try {
    decoded = decodeURIComponent(urlPath.split("?")[0]);
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

/** The server (not listening yet); `log(entry)` gets {method, path, status}. */
export function createUpdateServer({ root, log = () => {} }) {
  return createServer((req, res) => {
    const finish = (status, headers = {}, body = null) => {
      log({ method: req.method, path: req.url, status });
      res.writeHead(status, headers);
      res.end(body);
    };
    if (req.method !== "GET" && req.method !== "HEAD") {
      return finish(405, { allow: "GET, HEAD" });
    }
    const file = resolveUnder(root, req.url ?? "/");
    let stat = null;
    try {
      stat = file ? statSync(file) : null;
    } catch {
      stat = null;
    }
    if (!stat?.isFile()) return finish(404, { "content-type": "text/plain" }, "not found");
    const headers = {
      "content-type": TYPES[path.extname(file).toLowerCase()] ?? "application/octet-stream",
      "content-length": String(stat.size),
      "cache-control": "no-store",
    };
    log({ method: req.method, path: req.url, status: 200 });
    res.writeHead(200, headers);
    if (req.method === "HEAD") return res.end();
    createReadStream(file).pipe(res);
  });
}

/** Starts it on 127.0.0.1:`port` (0 = any free port); resolves the port. */
export function listenLoopback(server, port) {
  return new Promise((resolve, reject) => {
    server.once("error", reject);
    server.listen(port, "127.0.0.1", () => resolve(server.address().port));
  });
}
