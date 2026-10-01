# ADR 0057: No non-loopback sockets

Status: accepted · 2026-10-01 · post-M2 fix

## Context

Windows Defender Firewall showed "allow this app" prompts while Nail and its
tests ran. The rules Windows created from the answers named only three kinds
of programs: `target*/debug/deps/discover-<hash>.exe` (nail-discovery
tests), `target*/debug/deps/engines-<hash>.exe` (nail-core engine tests) and
`target*/debug/nail.exe` (`nail serve --seed demo-multi` in e2e) — 74
programs, 148 inbound rules across the build directories. `nail-app.exe`
never prompted.

Every TCP listener already bound `127.0.0.1`. The prompts came from UDP
sockets bound to the unspecified address — Windows asks when a program binds
a socket that can receive from the network (a TCP listener or a UDP socket on
a non-loopback or unspecified address). Outbound TCP connects do not prompt.

1. **hickory-resolver's DNS client sockets.** `HickoryDns` (nail-discovery)
   asks for SRV / MX records with hickory's own UDP sockets. Without a
   `ConnectionConfig::bind_addr`, hickory binds `0.0.0.0:<random>` or
   `[::]:<random>` (`hickory-net-0.26.3/src/udp/udp_stream.rs:218-222`;
   the same default for QUIC / H3 in
   `hickory-resolver-0.26.3/src/connection_provider.rs:144-146, 170-172`).
   This happened with the system configuration (`HickoryDns::system`, used by
   `Discoverer::system` when a user adds an account) and with the loopback
   mock DNS server (`HickoryDns::with_name_server`: nail-discovery tests,
   `nail_core::demo::DemoMulti` → the engine tests and `--seed demo-multi`).
2. **The remote UI's LAN address.** `remote_ui.rs` `lan_address` bound a
   `std::net::UdpSocket` to `0.0.0.0:0` / `[::]:0` and connected it to a
   documentation address to read the source address of the default route.
   It ran only for an enabled remote UI listening on every address.

A prompt is tied to the exact exe path, so hashed test binaries prompt again
after every rebuild, and users cannot meaningfully allow or deny them.

The rest of the workspace was checked: every listener (`nail-server`
`Server::bind`, `nail-mcp`, `nail-auth`'s OAuth2 loopback, the mock servers in
`nail-testkit` / `nail-testkit-net` / `nail-proxy::testing`, the desktop's
port probe) binds `127.0.0.1`; name resolution elsewhere (`nail-proxy`,
`reqwest`, `nail mcp`) uses `getaddrinfo` (the OS resolver, no socket of
Nail's); lettre, reqwest and the IMAP / POP3 / Sieve clients only connect.
The only intended exception is the remote UI listener (`Server::bind_with`
on the user's `bind_address`, plus its sandbox port on the same address).

## Decision

**Nail and its tests never open a socket on a non-loopback or unspecified
local address (TCP or UDP), except the remote UI listener the user enables.**

- **DNS goes through the OS resolver.** `nail_discovery::SystemDns`
  implements `DnsLookup` with `DnsQuery_W` (dnsapi, `windows` crate feature
  `Win32_NetworkManagement_Dns`) on Windows: the DNS Client service sends
  the query, Nail owns no DNS socket. Options: no LLMNR / NetBIOS
  (`DNS_QUERY_NO_MULTICAST | DNS_QUERY_NO_NETBT`); name errors and "no
  records" are empty answers; authority / additional records and other
  types (a CNAME on the way) are skipped. The query blocks, so it runs on
  tokio's blocking pool; discovery's per-source timeout still bounds it.
  `Discoverer::system` uses it. Other platforms keep hickory with the
  system configuration (`HickoryDns::system`, now `cfg(not(windows))`)
  until they get their own OS resolver path. Only SRV and MX are looked up
  anywhere in Nail; TXT is not used.
- **Mock DNS stays on loopback.** `HickoryDns::with_name_server` accepts
  only a loopback name server and binds its UDP socket to the same loopback
  address (`127.0.0.1:0` / `[::1]:0`, hickory picks the port). A unit test
  checks the configuration (`mock_name_servers_are_asked_from_loopback_only`).
- **LAN address without a socket.** `nail-server/src/lan.rs` reads the
  network interfaces (`GetAdaptersAddresses` through `ipconfig` 0.3.4,
  MIT / Apache-2.0, already in the lock through hickory) and picks what the
  default route would use: an up, non-loopback interface with a gateway of
  the family and the lowest metric, else any up interface; never loopback,
  link-local, unspecified or multicast. It runs only when an enabled remote
  UI listens on `0.0.0.0` / `::`. Other platforms list no LAN address for now
  (the computer's name is still offered).
- **Guard.** `scripts/check-socket-binds.mjs` (rules and allowlist in
  `scripts/lib/socket-binds.mjs`, tests in `scripts/test/socket-binds.test.mjs`)
  scans `crates/`, `app/` and `e2e/` for unspecified addresses (`0.0.0.0`,
  `[::]`, `"::"`, `[0, 0, 0, 0]`, `Ipv4Addr::UNSPECIFIED`,
  `Ipv6Addr::UNSPECIFIED`, …) and for binds / listens / hickory's system
  resolver with no loopback address on the same line. Each finding must
  match a reasoned allowlist entry (the remote UI listener, refusal tests,
  settings values, the proxy's blocked ranges); unused entries fail too.
  It runs first in `npm test` (CI `build-test`) and as a script test.

## Consequences

- Tests and seeded runs no longer prompt; neither does account discovery in
  the desktop app. Only enabling the remote UI on a non-loopback address
  may prompt once, for the installed `Nail.exe` (design: "No firewall
  changes at install").
- The `DnsQuery_W` path has no network test in CI (an ignored test,
  `system_dns_answers_real_queries`, runs it on demand); its record parsing
  is unit-tested on hand-built record lists. If the DNS Client service is
  disabled, Windows resolves in-process; that is the OS's choice, not a
  socket Nail configures.
- New engines, mock servers and test helpers bind `127.0.0.1` / `::1`; a
  deliberate exception needs an allowlist entry with its reason.
- Developers can remove the rules left by earlier builds (docs/dev-setup.md,
  "Sockets and the Windows firewall").

## Verification

In a fresh target directory: the nail-discovery tests, the nail-core engine
tests, `cargo test --workspace`, `nail serve --seed demo-multi` and
`npm run e2e` created no firewall rule for any of its binaries. While
`nail serve --seed demo-multi` ran, its listeners were all on `127.0.0.1`
(UI, MCP, mock IMAP / POP3 / SMTP / HTTP / Sieve servers) and its only UDP
endpoint was the mock DNS server on `127.0.0.1`. `Get-NetTCPConnection`
also lists a `Bound` row on `0.0.0.0` / `::` for each *outbound* connection
(same port as its `Established` `127.0.0.1` row); Windows reports every
connecting socket that way, it is not a listener and does not prompt.
