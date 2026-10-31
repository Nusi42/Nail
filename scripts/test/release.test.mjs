// Tests of scripts/lib/release.mjs. Run: node --test scripts/test/
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";

import { repoRoot } from "../lib/program.mjs";
import {
  ADVISORY_CHECKS,
  CHANGELOG,
  RELEASE_NOTES,
  RELEASE_PUBKEY,
  absoluteLinks,
  dependabotAlertsArgs,
  downloadBase,
  githubNotes,
  githubReleaseCommands,
  githubSlug,
  isPrerelease,
  openAlerts,
  plainLinks,
  releasePage,
  releaseSection,
  releasedChangelog,
  unwrapped,
  updateBase,
  updateManifestUrl,
  updateNotes,
} from "../lib/release.mjs";
import { MANIFEST_FILE } from "../lib/update-manifest.mjs";
import { CARGO_TOML, workspaceRepository, workspaceVersion } from "../lib/version.mjs";

const REPO = "https://github.com/example/nail";

/** @param {string} rel */
const read = (rel) => readFileSync(path.join(repoRoot, rel), "utf8");

const NOTES = `# Release notes

Intro of the page.

## Nail 0.9 (\`0.9.0\`)

A preview: keep a [backup](backup-restore.md) of your data.
Second line.

### New in this version

- See [Windows](windows.md#moving) and [the site](https://example.org/).

## Nail 0.8 (\`0.8.0\`)

Older.
`;

test("the current version has its section in the release notes", () => {
  const version = workspaceVersion(read(CARGO_TOML));
  assert.ok(releaseSection(read(RELEASE_NOTES), version), `${RELEASE_NOTES} has ${version}`);
});

test("a version's section ends at the next version", () => {
  const section = releaseSection(NOTES, "0.9.0") ?? "";
  assert.ok(section.startsWith("A preview:"));
  assert.ok(section.endsWith("[the site](https://example.org/)."));
  assert.equal(releaseSection(NOTES, "0.8.0"), "Older.");
  assert.equal(releaseSection(NOTES, "0.9"), null, "the version in full");
  assert.equal(releaseSection(NOTES, "1.0.0"), null);
});

test("the GitHub release links into the tagged sources", () => {
  const section = releaseSection(NOTES, "0.9.0") ?? "";
  const notes = githubNotes(section, REPO, "0.9.0");
  assert.match(
    notes,
    /\[backup\]\(https:\/\/github\.com\/example\/nail\/blob\/v0\.9\.0\/userDocs\/en\/backup-restore\.md\)/,
  );
  assert.match(
    notes,
    /\(https:\/\/github\.com\/example\/nail\/blob\/v0\.9\.0\/userDocs\/en\/windows\.md#moving\)/,
  );
  assert.match(notes, /\[the site\]\(https:\/\/example\.org\/\)/, "absolute links stay");
  assert.equal(absoluteLinks("[a](#top) [b](/x)", "B"), "[a](#top) [b](/x)");
});

test("paragraphs and list items are one line each, code stays", () => {
  const text = [
    "A paragraph",
    "wrapped.",
    "",
    "- An item",
    "  wrapped too.",
    "- Next.",
    "",
    "### Head",
    "",
    "```text",
    "a",
    "b",
    "```",
  ].join("\n");
  assert.equal(
    unwrapped(text),
    "A paragraph wrapped.\n\n- An item wrapped too.\n- Next.\n\n### Head\n\n```text\na\nb\n```",
  );
});

test("the update dialog gets the introduction as plain text", () => {
  const section = releaseSection(NOTES, "0.9.0") ?? "";
  assert.equal(
    updateNotes(section, releasePage(REPO, "0.9.0")),
    "A preview: keep a backup of your data. Second line.\n\nAll changes: https://github.com/example/nail/releases/tag/v0.9.0",
  );
  assert.equal(plainLinks("[a](b) and [c](d)"), "a and c");
});

test("the changelog's unreleased entries move under the version", () => {
  const before =
    "# Changelog\n\n## [Unreleased]\n\n### Fixed\n\n- A bug.\n\n## [0.1.0] — 2026-01-01\n\n[0.1.0]: x\n";
  const page = releasePage(REPO, "0.2.0");
  const after = releasedChangelog(before, { version: "0.2.0", date: "2026-10-25", page });
  assert.equal(
    after,
    `# Changelog\n\n## [Unreleased]\n\n## [0.2.0] — 2026-10-25\n\n### Fixed\n\n- A bug.\n\n## [0.1.0] — 2026-01-01\n\n[0.2.0]: ${page}\n[0.1.0]: x\n`,
  );
  assert.equal(releasedChangelog(after, { version: "0.2.0", date: "2027-01-01", page }), after);
  assert.throws(() => releasedChangelog("# Changelog\n", { version: "0.2.0", date: "d", page }));
  assert.ok(read(CHANGELOG).includes("## [Unreleased]\n"), `${CHANGELOG} keeps the heading`);
});

test("the app asks one address: latest.json of GitHub's latest release", () => {
  // The app reads <update base>/latest.json (scripts/lib/build.mjs); GitHub
  // serves the asset <name> of the release marked Latest at
  // <repository>/releases/latest/download/<name>.
  assert.equal(updateBase(REPO), `${REPO}/releases/latest/download`);
  assert.equal(updateManifestUrl(REPO), `${REPO}/releases/latest/download/${MANIFEST_FILE}`);
  assert.equal(updateManifestUrl(REPO), `${updateBase(REPO)}/latest.json`);
  // The manifest points at the version's own downloads.
  assert.equal(downloadBase(REPO, "0.9.0"), `${REPO}/releases/download/v0.9.0`);
});

test("a pre-release is a -beta… version", () => {
  assert.equal(isPrerelease("0.9.0"), false);
  assert.equal(isPrerelease("0.9.0-beta.1"), true);
  assert.throws(() => isPrerelease("nine"));
});

test("publishing: the release holds latest.json and is marked Latest", () => {
  const release = {
    slug: "example/nail",
    files: ["x64/Nail_0.9.0_x64-setup.exe", "x64/Nail_0.9.0_x64-setup.exe.sig"],
    manifest: "updates/latest.json",
    notes: "notes.md",
  };
  assert.deepEqual(githubReleaseCommands({ ...release, version: "0.9.0", exists: false }), [
    [
      "release",
      "create",
      "v0.9.0",
      "x64/Nail_0.9.0_x64-setup.exe",
      "x64/Nail_0.9.0_x64-setup.exe.sig",
      "updates/latest.json",
      "--repo",
      "example/nail",
      "--verify-tag",
      "--title",
      "Nail 0.9.0",
      "--notes-file",
      "notes.md",
      "--latest",
    ],
  ]);
  // A re-run replaces the files and marks the release Latest again.
  assert.deepEqual(githubReleaseCommands({ ...release, version: "0.9.0", exists: true }), [
    [
      "release",
      "upload",
      "v0.9.0",
      "x64/Nail_0.9.0_x64-setup.exe",
      "x64/Nail_0.9.0_x64-setup.exe.sig",
      "updates/latest.json",
      "--repo",
      "example/nail",
      "--clobber",
    ],
    ["release", "edit", "v0.9.0", "--repo", "example/nail", "--latest"],
  ]);
  // Only the version's own release: no channel releases any more (ADR 0210).
  for (const exists of [false, true]) {
    for (const args of githubReleaseCommands({ ...release, version: "0.9.0", exists })) {
      assert.equal(args[2], "v0.9.0");
    }
  }
});

test("a pre-release is never marked Latest, so it is never offered", () => {
  const release = {
    slug: "example/nail",
    files: ["x64/Nail_1.0.0-beta.1_x64-setup.exe"],
    manifest: "updates/latest.json",
    notes: "notes.md",
    version: "1.0.0-beta.1",
  };
  const [create, ...rest] = githubReleaseCommands({ ...release, exists: false });
  assert.deepEqual(rest, []);
  assert.ok(create);
  assert.ok(create.includes("updates/latest.json"));
  assert.deepEqual(create.slice(-2), ["--prerelease", "--latest=false"]);
  assert.ok(!create.includes("--latest"));
  const again = githubReleaseCommands({ ...release, exists: true });
  assert.equal(again.length, 1);
  assert.ok(!again.flat().some((arg) => arg.startsWith("--latest")));
});

test("the repository is a GitHub repository with a release key", () => {
  const repository = workspaceRepository(read(CARGO_TOML));
  assert.match(githubSlug(repository), /^[^/]+\/[^/]+$/);
  assert.equal(githubSlug("https://github.com/a/b.git"), "a/b");
  assert.throws(() => githubSlug("https://example.org/a/b"));
  assert.match(read(RELEASE_PUBKEY), /^[A-Za-z0-9+/=]+\s*$/, "the public half only");
});

test("a release checks the advisories of every database with network", () => {
  const commands = ADVISORY_CHECKS.map((c) => [c.command, ...c.args].join(" "));
  assert.deepEqual(commands, ["npm run check -- --advisories", "npm audit --omit=dev"]);
  const source = readFileSync(path.join(repoRoot, "scripts/release.mjs"), "utf8");
  assert.match(source, /\.\.\.ADVISORY_CHECKS/);
  assert.match(source, /openAlerts\(/);
  assert.deepEqual(dependabotAlertsArgs("example/nail"), [
    "api",
    "--paginate",
    "--jq",
    ".[]",
    "repos/example/nail/dependabot/alerts?state=open&per_page=100",
  ]);
});

test("open Dependabot alerts are listed one per line", () => {
  assert.deepEqual(openAlerts(""), []);
  assert.deepEqual(openAlerts("\n  \n"), []);
  /**
   * @param {number} number
   * @param {string | null} scope
   */
  const alert = (number, scope) =>
    JSON.stringify({
      number,
      dependency: {
        package: { ecosystem: "rust", name: "zip" },
        manifest_path: "Cargo.lock",
        scope,
      },
      security_advisory: { ghsa_id: "GHSA-xxxx-yyyy-zzzz", severity: "high", summary: "Bad" },
    });
  assert.deepEqual(openAlerts(`${alert(9, "runtime")}\n${alert(10, null)}\n`), [
    "#9 high rust zip (Cargo.lock, runtime): GHSA-xxxx-yyyy-zzzz Bad",
    "#10 high rust zip (Cargo.lock, unknown): GHSA-xxxx-yyyy-zzzz Bad",
  ]);
  assert.throws(() => openAlerts("not json"));
});
