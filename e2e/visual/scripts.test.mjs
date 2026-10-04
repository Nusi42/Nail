// node --test e2e/visual/scripts.test.mjs — the runner, the theme probe's
// and the self-test's argument handling and checks (the harness itself is
// exercised end to end by selftest.mjs).
import assert from "node:assert/strict";
import { createServer } from "node:http";
import path from "node:path";
import { test } from "node:test";
import { parseArgs as probeArgs, rpc } from "./lib/probe-themes.mjs";
import { parseArgs, playwrightArgs } from "./run-visual.mjs";
import { checkStep } from "./selftest.mjs";

test("runner arguments", () => {
  assert.deepEqual(parseArgs([]), { update: false, review: true, playwright: [] });
  assert.deepEqual(parseArgs(["--update", "--no-review", "--", "--headed", "-g", "frost"]), {
    update: true,
    review: false,
    playwright: ["--headed", "-g", "frost"],
  });
  assert.throws(() => parseArgs(["--all"]), /unknown argument --all/);
  const config = path.join("e2e", "visual", "playwright.visual.config.ts");
  assert.deepEqual(playwrightArgs(parseArgs([])), ["test", "-c", config]);
  // Updating writes only new and changed baselines.
  assert.deepEqual(playwrightArgs(parseArgs(["--update", "--", "harness"])), [
    "test",
    "-c",
    config,
    "--update-snapshots=changed",
    "harness",
  ]);
});

test("probe arguments", () => {
  assert.deepEqual(probeArgs(["--bin", "nail.exe", "--ui-dir", "dist"]), {
    bin: "nail.exe",
    uiDir: "dist",
    timeoutMs: 60_000,
  });
  assert.equal(probeArgs(["--bin", "n", "--ui-dir", "d", "--timeout-ms", "500"]).timeoutMs, 500);
  assert.throws(() => probeArgs(["--ui-dir", "d"]), /--bin is required/);
  assert.throws(() => probeArgs(["--bin", "n"]), /--ui-dir is required/);
  assert.throws(() => probeArgs(["--bin", "n", "--ui-dir", "d", "--timeout-ms", "x"]), /positive/);
  assert.throws(() => probeArgs(["--port", "1"]), /unknown argument/);
});

test("the probe's request fails cleanly when nothing answers on the socket", async () => {
  // A loopback HTTP server on a free port that refuses the WebSocket upgrade.
  const server = createServer((_req, res) => {
    res.writeHead(404).end();
  });
  server.on("upgrade", (_req, socket) => {
    socket.destroy();
  });
  await new Promise((resolve) => {
    server.listen(0, "127.0.0.1", () => {
      resolve(undefined);
    });
  });
  try {
    const address = server.address();
    const port = typeof address === "object" && address !== null ? address.port : 0;
    await assert.rejects(
      rpc(`http://127.0.0.1:${String(port)}/`, { type: "ListThemes" }, 5_000),
      /WebSocket error/,
    );
  } finally {
    await new Promise((resolve) => {
      server.close(resolve);
    });
  }
});

test("self-test steps compare exit codes and per-screen statuses", () => {
  const summary = {
    exitCode: 1,
    entries: [
      { variant: "current-comfortable", screen: "main", status: "changed", message: "diff" },
      { variant: "current-comfortable", screen: "menu", status: "passed", message: null },
      { variant: "frost-comfortable", screen: "dialog", status: "passed", message: null },
    ],
  };
  assert.deepEqual(
    checkStep("4", summary, { exitCode: 1, statuses: { main: "changed", menu: "passed" } }),
    [],
  );
  assert.deepEqual(
    checkStep("3", summary, { exitCode: 0, statuses: { main: "passed", dialog: "passed" } }),
    [
      "3: exit code 1, expected 0",
      "3: main is changed, expected passed (diff)",
      "3: no result for dialog",
    ],
  );
});
