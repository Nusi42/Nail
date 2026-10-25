// Tests of scripts/lib/release.mjs. Run: node --test scripts/test/
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import path from "node:path";
import { test } from "node:test";

import { repoRoot } from "../lib/program.mjs";
import {
  CHANGELOG,
  RELEASE_NOTES,
  RELEASE_PUBKEY,
  absoluteLinks,
  channelAsset,
  channelOf,
  downloadBase,
  githubNotes,
  githubSlug,
  plainLinks,
  releasePage,
  releaseSection,
  releasedChangelog,
  unwrapped,
  updateBase,
  updateNotes,
} from "../lib/release.mjs";
import { CHANNELS, manifestPath } from "../lib/update-manifest.mjs";
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

test("each channel's manifest is the latest.json of the release named like the channel", () => {
  // The app reads <base>/<channel>/latest.json; a release's asset is
  // <repository>/releases/download/<tag>/<name>.
  for (const channel of CHANNELS) {
    const { tag, name } = channelAsset(channel);
    assert.equal(`${tag}/${name}`, manifestPath(channel));
    assert.equal(
      `${updateBase(REPO)}/${tag}/${name}`,
      `${REPO}/releases/download/${manifestPath(channel)}`,
    );
  }
  assert.equal(downloadBase(REPO, "0.9.0"), `${REPO}/releases/download/v0.9.0`);
});

test("a pre-release goes to beta, a release to stable", () => {
  assert.equal(channelOf("0.9.0"), "stable");
  assert.equal(channelOf("0.9.0-beta.1"), "beta");
  assert.throws(() => channelOf("nine"));
});

test("the repository is a GitHub repository with a release key", () => {
  const repository = workspaceRepository(read(CARGO_TOML));
  assert.match(githubSlug(repository), /^[^/]+\/[^/]+$/);
  assert.equal(githubSlug("https://github.com/a/b.git"), "a/b");
  assert.throws(() => githubSlug("https://example.org/a/b"));
  assert.match(read(RELEASE_PUBKEY), /^[A-Za-z0-9+/=]+\s*$/, "the public half only");
});
