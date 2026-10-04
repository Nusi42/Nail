/**
 * The "no non-loopback sockets" check (ADR 0057): Nail and its tests never
 * open a socket on a non-loopback or unspecified local address, except the
 * remote UI listener the user enables. A UDP bind or a listener on
 * `0.0.0.0` / `[::]` makes Windows Defender Firewall prompt, per exe path.
 *
 * Two kinds of findings, both on non-comment source lines:
 * - an unspecified address (`0.0.0.0`, `[::]`, `"::"`, `[0, 0, 0, 0]`,
 *   `Ipv4Addr::UNSPECIFIED`, `Ipv6Addr::UNSPECIFIED`, `INADDR_ANY`, …);
 * - a socket bind / listen (`UdpSocket::bind(`, `TcpListener::bind(`,
 *   `TcpSocket`, socket2 `Socket::new(`, Node `.listen(`, hickory's
 *   system-configured `builder_tokio(`) with no loopback address on the
 *   same line.
 *
 * Every finding must match an ALLOWLIST entry (file + text on the line +
 * reason); allowlist entries that match nothing are reported as stale.
 * Pure helpers here; the CLI is scripts/check-socket-binds.mjs, the tests
 * scripts/test/socket-binds.test.mjs.
 */
import fs from "node:fs";
import path from "node:path";

/**
 * A suspicious line: `kind` is "unspecified address" or "bind without
 * loopback", `name` the pattern, `text` the trimmed line (1-based `line`).
 *
 * @typedef {{ line: number, kind: string, name: string, text: string }} Finding
 */

/**
 * A known finding that is not a socket on a non-loopback address.
 *
 * @typedef {{ file: string, text: string, reason: string }} AllowEntry
 */

/** Directories scanned, relative to the repository root. */
export const ROOTS = ["crates", "app", "e2e"];

/** File extensions scanned. */
export const EXTENSIONS = new Set([".rs", ".ts", ".tsx", ".mjs", ".js", ".cjs", ".json", ".toml"]);

/** Directory names never descended into. */
const SKIP_DIRS = new Set([
  "node_modules",
  "target",
  "dist",
  "gen",
  "icons",
  "corpus",
  "test-results",
  "playwright-report",
  ".git",
]);

/**
 * Unspecified ("any") addresses.
 *
 * @type {{ name: string, re: RegExp }[]}
 */
const UNSPECIFIED = [
  { name: "0.0.0.0", re: /(?<![\d.])0\.0\.0\.0(?![\d])/ },
  { name: "[::]", re: /\[::\]/ },
  { name: '"::"', re: /(["'`])::\1/ },
  { name: "[0, 0, 0, 0]", re: /\[\s*0\s*,\s*0\s*,\s*0\s*,\s*0\s*\]/ },
  { name: "Ipv4Addr::new(0, 0, 0, 0)", re: /Ipv4Addr::new\(\s*0\s*,\s*0\s*,\s*0\s*,\s*0\s*\)/ },
  { name: "UNSPECIFIED", re: /Ipv[46]Addr::UNSPECIFIED/ },
  { name: "INADDR_ANY", re: /\bINADDR_ANY\b|\bin6addr_any\b|\bIN6ADDR_ANY/i },
];

/**
 * Socket binds that must name a loopback address on the same line.
 *
 * @type {{ name: string, re: RegExp, only?: RegExp }[]}
 */
const BINDS = [
  { name: "UdpSocket::bind", re: /UdpSocket::bind\s*\(/ },
  { name: "TcpListener::bind", re: /TcpListener::bind\s*\(/ },
  { name: "TcpSocket", re: /TcpSocket::new_v[46]\s*\(|\.bind\(\s*SocketAddr/ },
  { name: "Socket::new", re: /\bSocket::new\s*\(/ },
  { name: ".listen(", re: /\.listen\s*\(/, only: /\.(?:[cm]?js|tsx?)$/ },
  { name: "hickory system resolver", re: /builder_tokio\s*\(/ },
];

const LOOPBACK = /127\.0\.0\.1|::1\b|LOCALHOST|localhost/;

/**
 * Known findings that are not a socket on a non-loopback address.
 * `file` is relative to the repository root with forward slashes; `text`
 * must occur on the finding's line.
 *
 * @type {AllowEntry[]}
 */
export const ALLOWLIST = [
  // The remote UI listener, enabled by the user (ADR 0041). Server::bind
  // refuses non-loopback addresses; bind_with is the remote UI's.
  {
    file: "crates/nail-server/src/lib.rs",
    text: "TcpListener::bind(addr)",
    reason: "Server::bind is loopback-only; bind_with serves the user-enabled remote UI",
  },
  // Test helper: callers pass loopback (start() is 127.0.0.1:0, tests [::1]:0).
  {
    file: "crates/nail-proxy/src/testing.rs",
    text: "TcpListener::bind(addr)",
    reason: "test upstream; every caller passes a loopback address",
  },
  // hickory with the system configuration: the non-Windows fallback only.
  {
    file: "crates/nail-discovery/src/net.rs",
    text: "Resolver::builder_tokio()",
    reason: "non-Windows fallback (#[cfg(not(windows))]); Windows uses DnsQuery_W",
  },
  // Tests that a non-loopback name server is refused (no socket).
  {
    file: "crates/nail-discovery/src/net.rs",
    text: '"0.0.0.0:53",',
    reason: "test: refused name server",
  },
  {
    file: "crates/nail-discovery/src/net.rs",
    text: '"[::]:53",',
    reason: "test: refused name server",
  },
  // Settings values and their validation (strings, no socket).
  {
    file: "crates/nail-api/src/protocol.rs",
    text: '"bind_address": "0.0.0.0"',
    reason: "protocol fixture",
  },
  { file: "crates/nail-api/src/remote.rs", text: '"::",', reason: "test: address classification" },
  {
    file: "crates/nail-api/src/remote.rs",
    text: '"0.0.0.0",',
    reason: "test: address classification",
  },
  { file: "crates/nail-api/src/remote.rs", text: '"0.0.0.0/0"', reason: "test: network ranges" },
  {
    file: "crates/nail-api/src/remote.rs",
    text: 'bind_address: "0.0.0.0".into()',
    reason: "test: settings validation",
  },
  {
    file: "crates/nail-core/tests/service.rs",
    text: 'bind_address: "0.0.0.0".into()',
    reason: "test: stored settings (no host runs)",
  },
  {
    file: "crates/nail-store/tests/remote.rs",
    text: 'bind_address: "0.0.0.0".into()',
    reason: "test: stored settings",
  },
  {
    file: "crates/nail-server/src/acme.rs",
    text: 'bind_address: "0.0.0.0".into()',
    reason: "test: ACME settings (locked)",
  },
  {
    file: "crates/nail-server/src/cert.rs",
    text: 'bind_address = "0.0.0.0".into()',
    reason: "test: certificate names",
  },
  {
    file: "crates/nail-server/src/tls.rs",
    text: '"0.0.0.0/0"',
    reason: "test: peer network ranges",
  },
  // Refusals: binding a non-loopback address without TLS is refused before any socket.
  {
    file: "crates/nail-server/tests/server.rs",
    text: "SocketAddr::from(([0, 0, 0, 0], 0))",
    reason: "test: Server::bind refuses it (NotLoopback) before binding",
  },
  {
    file: "crates/nail-server/tests/transport.rs",
    text: "SocketAddr::from(([0, 0, 0, 0], 0))",
    reason: "test: bind / bind_with refuse it before binding",
  },
  // The SSRF proxy's blocked ranges and their tests (destinations, not binds).
  {
    file: "crates/nail-proxy/src/policy.rs",
    text: "(Ipv4Addr::UNSPECIFIED, 8)",
    reason: "blocked range",
  },
  { file: "crates/nail-proxy/src/policy.rs", text: '"0.0.0.0",', reason: "test: blocked address" },
  { file: "crates/nail-proxy/src/policy.rs", text: '"::",', reason: "test: blocked address" },
  {
    file: "crates/nail-proxy/tests/fetch.rs",
    text: '"http://0.0.0.0/"',
    reason: "test: refused URL",
  },
  { file: "crates/nail-proxy/tests/fetch.rs", text: '"http://[::]/"', reason: "test: refused URL" },
];

/**
 * Whether a source line is a comment (Rust / TS / JS line or block comment).
 *
 * @param {string} line
 */
export function isComment(line) {
  const t = line.trim();
  return t.startsWith("//") || t.startsWith("/*") || t.startsWith("*");
}

/**
 * Findings of one file's text: `{ line, kind, name, text }` (1-based line).
 * `file` selects the language-specific patterns.
 *
 * @param {string} text
 * @param {string} [file]
 * @returns {Finding[]}
 */
export function scanText(text, file = "") {
  /** @type {Finding[]} */
  const findings = [];
  text.split(/\r?\n/).forEach((raw, index) => {
    if (isComment(raw)) return;
    const line = raw.replace(/\s\/\/.*$/, "");
    for (const p of UNSPECIFIED) {
      if (p.re.test(line)) {
        findings.push({
          line: index + 1,
          kind: "unspecified address",
          name: p.name,
          text: raw.trim(),
        });
      }
    }
    for (const p of BINDS) {
      if (p.only && !p.only.test(file)) continue;
      if (p.re.test(line) && !LOOPBACK.test(line)) {
        findings.push({
          line: index + 1,
          kind: "bind without loopback",
          name: p.name,
          text: raw.trim(),
        });
      }
    }
  });
  return findings;
}

/**
 * Every scanned file under `root` (relative paths with forward slashes).
 *
 * @param {string} root
 * @param {string[]} [roots]
 * @returns {string[]}
 */
export function listFiles(root, roots = ROOTS) {
  /** @type {string[]} */
  const out = [];
  /** @param {string} dir */
  const walk = (dir) => {
    let entries;
    try {
      entries = fs.readdirSync(dir, { withFileTypes: true });
    } catch {
      return;
    }
    for (const e of entries) {
      const full = path.join(dir, e.name);
      if (e.isDirectory()) {
        if (!SKIP_DIRS.has(e.name)) walk(full);
      } else if (EXTENSIONS.has(path.extname(e.name))) {
        out.push(path.relative(root, full).split(path.sep).join("/"));
      }
    }
  };
  for (const r of roots) walk(path.join(root, r));
  return out.sort();
}

/**
 * Checks `files` (`[{ file, text }]`) against `allowlist`: findings no
 * entry allows (`violations`) and entries that allowed nothing (`stale`).
 *
 * @param {{ file: string, text: string }[]} files
 * @param {AllowEntry[]} [allowlist]
 * @returns {{ violations: (Finding & { file: string })[], stale: AllowEntry[] }}
 */
export function check(files, allowlist = ALLOWLIST) {
  /** @type {Set<number>} */
  const used = new Set();
  /** @type {(Finding & { file: string })[]} */
  const violations = [];
  for (const { file, text } of files) {
    for (const f of scanText(text, file)) {
      const entry = allowlist.findIndex((a) => a.file === file && f.text.includes(a.text));
      if (entry === -1) violations.push({ file, ...f });
      else used.add(entry);
    }
  }
  const stale = allowlist.filter((_, i) => !used.has(i));
  return { violations, stale };
}

/**
 * Runs the check over the repository at `root`.
 *
 * @param {string} root
 */
export function checkRepository(root) {
  const files = listFiles(root).map((file) => ({
    file,
    text: fs.readFileSync(path.join(root, file), "utf8"),
  }));
  return { files: files.length, ...check(files) };
}
