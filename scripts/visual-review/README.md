# scripts/visual-review — baseline review page (M5a WP154)

Slot of WP154 (contract ADR 0102): builds a local review page from a
visual-regression run of `e2e/visual/` (baseline, actual and difference per
screen × theme × density), so a human can accept or reject changed
baselines. Output goes to the build directory, never into the repository;
any server it starts binds 127.0.0.1 on a free port (CLAUDE.md: no fixed
development ports, no non-loopback sockets).
