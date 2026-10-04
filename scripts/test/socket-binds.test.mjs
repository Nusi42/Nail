// Tests of scripts/lib/socket-binds.mjs (ADR 0057). Run: node --test scripts/test/
import assert from "node:assert/strict";
import path from "node:path";
import { test } from "node:test";
import { fileURLToPath } from "node:url";

import { check, checkRepository, scanText } from "../lib/socket-binds.mjs";

/**
 * @param {string} text
 * @param {string} [file]
 */
const names = (text, file = "x.rs") => scanText(text, file).map((f) => f.name);

test("unspecified addresses are found in every spelling", () => {
  assert.deepEqual(names('let s = UdpSocket::bind("0.0.0.0:0")?;'), ["0.0.0.0", "UdpSocket::bind"]);
  assert.deepEqual(names('"[::]:0".parse()'), ["[::]"]);
  assert.deepEqual(names('let ip: IpAddr = "::".parse()?;'), ['"::"']);
  assert.deepEqual(names("SocketAddr::from(([0, 0, 0, 0], port))"), ["[0, 0, 0, 0]"]);
  assert.deepEqual(names("SocketAddr::new(Ipv4Addr::UNSPECIFIED.into(), 0)"), ["UNSPECIFIED"]);
  assert.deepEqual(names("SocketAddr::new(Ipv6Addr::UNSPECIFIED.into(), 0)"), ["UNSPECIFIED"]);
  assert.deepEqual(names("Ipv4Addr::new(0, 0, 0, 0)"), ["Ipv4Addr::new(0, 0, 0, 0)"]);
  // Not the any address.
  assert.deepEqual(names('"10.0.0.0/8" "100.0.0.0" "::1" "fe80::1"'), []);
});

test("binds must name a loopback address on the same line", () => {
  assert.deepEqual(names('UdpSocket::bind(("127.0.0.1", 0)).await?'), []);
  assert.deepEqual(names("TcpListener::bind(SocketAddr::from((Ipv4Addr::LOCALHOST, port)))"), []);
  assert.deepEqual(names("TcpListener::bind(addr).await?"), ["TcpListener::bind"]);
  assert.deepEqual(names("let socket = std::net::UdpSocket::bind(local).ok()?;"), [
    "UdpSocket::bind",
  ]);
  assert.deepEqual(names("let r = Resolver::builder_tokio()?;"), ["hickory system resolver"]);
  // Node servers: `.listen(port)` binds every address.
  assert.deepEqual(names("server.listen(0);", "a.ts"), [".listen("]);
  assert.deepEqual(names("server.listen(0, '127.0.0.1');", "a.mjs"), []);
  // `.listen(` in Rust is not a socket call.
  assert.deepEqual(names(".listen(key, &client.id)", "a.rs"), []);
});

test("comments are not findings", () => {
  assert.deepEqual(names("/// `0.0.0.0` / `::` = every address"), []);
  assert.deepEqual(names("// UdpSocket::bind(local)"), []);
  assert.deepEqual(names(' * listens on "0.0.0.0"'), []);
  assert.deepEqual(names('let a = "127.0.0.1"; // not 0.0.0.0'), []);
});

test("the allowlist allows by file and line text and reports stale entries", () => {
  const allow = [
    { file: "a.rs", text: "TcpListener::bind(addr)", reason: "r" },
    { file: "b.rs", text: "never there", reason: "r" },
  ];
  const result = check(
    [
      { file: "a.rs", text: "TcpListener::bind(addr).await?\nUdpSocket::bind(any)" },
      { file: "c.rs", text: "TcpListener::bind(addr).await?" },
    ],
    allow,
  );
  assert.deepEqual(
    result.violations.map((v) => `${v.file}:${String(v.line)}`),
    ["a.rs:2", "c.rs:1"],
  );
  assert.deepEqual(
    result.stale.map((s) => s.file),
    ["b.rs"],
  );
});

test("the repository opens no sockets on non-loopback addresses", () => {
  const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "../..");
  const { files, violations, stale } = checkRepository(root);
  assert.ok(files > 100, `scanned ${String(files)} files`);
  assert.deepEqual(violations, []);
  assert.deepEqual(stale, []);
});
