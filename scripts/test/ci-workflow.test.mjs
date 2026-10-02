// Checks of .github/workflows/ci.yml against the repository (M4b review):
// a desktop spec that skips itself in CI unless NAIL_E2E_DESKTOP is set
// must be run by the `desktop` job, or it never runs in CI at all.
// Run: node --test "scripts/test/*.test.mjs"
import assert from "node:assert/strict";
import { readdirSync, readFileSync } from "node:fs";
import { test } from "node:test";

const root = new URL("../../", import.meta.url);
const read = (relative) => readFileSync(new URL(relative, root), "utf8");

test("every desktop-gated Playwright spec runs in the desktop job", () => {
  const specs = readdirSync(new URL("e2e/tests/", root)).filter(
    (name) => name.endsWith(".spec.ts") && read(`e2e/tests/${name}`).includes("NAIL_E2E_DESKTOP"),
  );
  assert.ok(specs.includes("desktop.spec.ts"));
  assert.ok(specs.includes("desktop-m4b.spec.ts"));

  const ci = read(".github/workflows/ci.yml");
  const lines = ci.split(/\r?\n/);
  // The step that sets NAIL_E2E_DESKTOP and the command it runs.
  const envAt = lines.findIndex((l) => /^\s*NAIL_E2E_DESKTOP:/.test(l));
  assert.ok(envAt > 0, "the desktop job sets NAIL_E2E_DESKTOP");
  const run = lines
    .slice(0, envAt)
    .reverse()
    .find((l) => /^\s*run: npm run e2e:only/.test(l));
  assert.ok(run, "the desktop step runs Playwright");
  const filters = run.split("--")[1]?.trim().split(/\s+/) ?? [];
  for (const spec of specs) {
    assert.ok(filters.includes(`tests/${spec}`), `${spec} is not run by the desktop job: ${run}`);
  }
  // A filter that names no file would silently run nothing.
  for (const filter of filters) {
    assert.ok(specs.includes(filter.replace(/^tests\//, "")), `unknown spec ${filter}`);
  }
});

test("the e2e desktop build enables the test-only overrides, installers never do", () => {
  const desktop = read("e2e/scripts/build-desktop.mjs");
  // Each cargo call that needs the feature selects exactly one package and
  // names the feature plainly (cargo drops `pkg/feature` for several -p).
  const calls = [...desktop.matchAll(/execFileSync\(\s*"cargo",\s*\[([^\]]*)\]/g)].map((m) =>
    [...m[1].matchAll(/"([^"]*)"/g)].map((a) => a[1]),
  );
  const withFeature = (pkg) =>
    calls.find((args) => args[args.indexOf("-p") + 1] === pkg && args.includes("--features"));
  for (const pkg of ["nail-app", "nail-mapi"]) {
    const args = withFeature(pkg);
    assert.ok(args, `${pkg} is built with features`);
    assert.equal(args.filter((a) => a === "-p").length, 1, `${pkg}: one package per call`);
    const features = args[args.indexOf("--features") + 1].split(",");
    assert.ok(features.includes("test-overrides"), `${pkg}: ${features}`);
  }
  const installer = read("scripts/build-installer.mjs");
  assert.doesNotMatch(installer, /test-overrides/);
  const ci = read(".github/workflows/ci.yml");
  // The mapi job's release builds are what ships: no test features there.
  const releaseBuilds = ci
    .split(/\r?\n/)
    .filter((l) => /cargo build -p nail-mapi --release/.test(l));
  assert.ok(releaseBuilds.length >= 3);
  for (const line of releaseBuilds) assert.doesNotMatch(line, /--features|--all-features/);
  // And the job checks the built DLLs carry no trace of the variables.
  assert.match(ci, /name: No test overrides in release DLLs/);
  assert.match(ci, /'NAIL_MAPI_PIPE', 'NAIL_MAPI_APP'/);
});
