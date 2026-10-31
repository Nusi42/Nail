#!/usr/bin/env node
/**
 * Builds and publishes a release (ADR 0203; steps in docs/release.md):
 *
 *   npm run release:prepare -- --version 0.8.0 [--skip-checks] [--dry-run]
 *                                              [--dev-updater-key]
 *   npm run release:publish [-- --version 0.8.0] [--yes] [--remote github]
 *
 * prepare, on a clean checkout of the lead branch: sets the version in
 * every copy (scripts/lib/version.mjs), moves the changelog's
 * `[Unreleased]` entries under it, runs the local verification (unless
 * --skip-checks), builds the x64 NSIS installer and the portable
 * zip with update signatures, writes the channel's `latest.json` (every
 * signature checked against packaging/updater/release.pub) and the GitHub
 * release's text, then commits `chore(release): release X.Y.Z` and writes
 * `<build dir>/release/X.Y.Z/release.json`. The update key comes from
 * TAURI_SIGNING_PRIVATE_KEY_PATH (or _KEY) and _PASSWORD, set for this
 * command only; only the installer build sees it. --dry-run builds with
 * the given key (or --dev-updater-key) and commits nothing: the version
 * files are restored afterwards.
 *
 * publish, once the installer has been tried: tags the release commit
 * `vX.Y.Z`, pushes the branch and the tag to origin; with a master checkout
 * there it fast-forwards the master and the master pushes `main` and the
 * tag to the public remote (ADR 0205), else this checkout pushes the branch
 * as `main` and the tag, creates the GitHub release with every built file, and
 * replaces `latest.json` on the rolling release of the channel (and of
 * beta for a stable release, unless beta is newer). Every step checks
 * first, so a re-run after a failure goes on where it stopped. Needs the
 * GitHub CLI (`gh auth login` once).
 */
import { existsSync, mkdirSync, readFileSync, readdirSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";

import { forEachArg } from "./lib/args.mjs";
import { buildBase, withoutUpdaterSecrets } from "./lib/build.mjs";
import { readJson } from "./lib/json.mjs";
import { confirm, repoRoot, runMain } from "./lib/program.mjs";
import {
  CHANGELOG,
  RECORD_FILE,
  RELEASE_NOTES,
  RELEASE_PUBKEY,
  channelAsset,
  channelOf,
  downloadBase,
  githubNotes,
  githubSlug,
  releasePage,
  releaseSection,
  releaseTag,
  releasedChangelog,
  updateBase,
  updateNotes,
} from "./lib/release.mjs";
import { output, run } from "./lib/run.mjs";
import { CHANNELS, compareVersions, MANIFEST_FILE, parseVersion } from "./lib/update-manifest.mjs";
import { devKeyPaths } from "./lib/updater-key.mjs";
import { CARGO_TOML, setVersion, workspaceRepository } from "./lib/version.mjs";
import { masterOf, publishPushes } from "./lib/push-plan.mjs";
import { assertWorkFolder, originOf } from "./lib/work-folder.mjs";
import { writeUpdateManifests } from "./update-manifest.mjs";

/** The architecture and installers of a release (ADR 0203). */
const ARCH = "x64";
const BUNDLES = "nsis";

/**
 * The local verification before a release (docs/dev-setup.md
 * "Verification (no hosted CI)"), without the steps for builds a release
 * does not ship (ARM64, MSI) and the installer smoke tests and the perf
 * run, which stay by hand.
 *
 * @type {{ command: string, args: string[], env?: Record<string, string> }[]}
 */
const CHECKS = [
  { command: "npm", args: ["ci"] },
  { command: "npm", args: ["run", "check"] },
  { command: "npm", args: ["test"] },
  { command: "node", args: ["--test", "scripts/test/*.test.mjs"] },
  { command: "npm", args: ["run", "build"] },
  { command: "cargo", args: ["test", "--workspace", "-j", "4"] },
  { command: "npm", args: ["run", "e2e"] },
  { command: "npm", args: ["run", "visual"] },
  { command: "npm", args: ["run", "e2e:only", "--", "desktop"], env: { NAIL_E2E_DESKTOP: "1" } },
  { command: "cargo", args: ["test", "-p", "nail-mapi", "--target", "i686-pc-windows-msvc"] },
];

/** @param {string} rel */
const read = (rel) => readFileSync(path.join(repoRoot, rel), "utf8");

/** @param {string[]} args */
const git = (args) => output("git", args, { shell: false });

/** The public repository (`[workspace.package] repository`). */
function repository() {
  return workspaceRepository(read(CARGO_TOML));
}

/**
 * The release folder of a version (gitignored build output).
 *
 * @param {string} version
 */
function releaseDir(version) {
  return path.join(buildBase(process.env, repoRoot), "release", version);
}

/**
 * Today as `YYYY-MM-DD` (UTC).
 */
function today() {
  return new Date().toISOString().slice(0, 10);
}

/**
 * The options of `prepare`.
 *
 * @param {string[]} argv
 */
function prepareArgs(argv) {
  const o = { version: "", skipChecks: false, dryRun: false, devUpdaterKey: false };
  forEachArg(argv, (arg, { value }) => {
    if (arg === "--version") o.version = value();
    else if (arg === "--skip-checks") o.skipChecks = true;
    else if (arg === "--dry-run") o.dryRun = true;
    else if (arg === "--dev-updater-key") o.devUpdaterKey = true;
    else throw new Error(`unknown option ${arg}`);
  });
  if (!o.version) throw new Error("--version is required");
  parseVersion(o.version);
  if (o.devUpdaterKey && !o.dryRun) {
    throw new Error("--dev-updater-key is for a --dry-run only: a release is signed with its key");
  }
  return o;
}

/**
 * The update key for the installer build: the environment of that one
 * command and the public half the signatures are checked against.
 *
 * @param {boolean} dev
 */
function updateKey(dev) {
  if (dev) {
    // Only the committed public half is read; the build finds the private one.
    const { publicKey } = devKeyPaths({ keysDir: "", repoRoot });
    return {
      buildArgs: ["--dev-updater-key"],
      env: process.env,
      publicKey: readFileSync(publicKey, "utf8"),
    };
  }
  if (!process.env.TAURI_SIGNING_PRIVATE_KEY && !process.env.TAURI_SIGNING_PRIVATE_KEY_PATH) {
    throw new Error(
      "no update key: set TAURI_SIGNING_PRIVATE_KEY_PATH (and TAURI_SIGNING_PRIVATE_KEY_PASSWORD) for this command",
    );
  }
  const pubkeyFile = path.join(repoRoot, RELEASE_PUBKEY);
  return {
    buildArgs: [],
    env: { ...process.env, NAIL_UPDATER_PUBKEY_FILE: pubkeyFile, NAIL_UPDATER_PUBKEY: "" },
    publicKey: readFileSync(pubkeyFile, "utf8"),
  };
}

/** @param {string[]} argv */
function prepare(argv) {
  assertWorkFolder("npm run release:prepare");
  const o = prepareArgs(argv);
  const { version } = o;
  const tag = releaseTag(version);
  const repo = repository();
  if (git(["status", "--porcelain"])) throw new Error("the checkout has uncommitted files");
  if (git(["tag", "--list", tag])) throw new Error(`${tag} exists already`);
  const section = releaseSection(read(RELEASE_NOTES), version);
  if (!section)
    throw new Error(`${RELEASE_NOTES} has no section for ${version} (## … (\`${version}\`))`);
  const key = updateKey(o.devUpdaterKey);

  // The version and the changelog; Cargo.lock follows the workspace.
  const changed = setVersion(repoRoot, version);
  run("cargo", ["update", "--workspace"], { shell: false });
  const page = releasePage(repo, version);
  const changelog = read(CHANGELOG);
  writeFileSync(
    path.join(repoRoot, CHANGELOG),
    releasedChangelog(changelog, { version, date: today(), page }),
  );
  const restore = () => {
    run("git", ["checkout", "--", "."], { shell: false });
  };

  try {
    if (!o.skipChecks) {
      for (const check of CHECKS) {
        const env = check.env && { ...withoutUpdaterSecrets(process.env), ...check.env };
        run(check.command, check.args, env ? { env } : {});
      }
    }
    const dir = releaseDir(version);
    rmSync(dir, { recursive: true, force: true });
    const artifacts = path.join(dir, ARCH);
    run(
      "node",
      [
        "scripts/build-installer.mjs",
        "--arch",
        ARCH,
        "--bundles",
        BUNDLES,
        "--out",
        artifacts,
        "--update-base",
        updateBase(repo),
        ...key.buildArgs,
      ],
      // The one command that gets the update key; it passes it on only to
      // the bundler.
      { env: key.env, shell: false },
    );
    const channel = channelOf(version);
    const updates = path.join(dir, "updates");
    writeUpdateManifests({
      version,
      channel,
      artifacts: [artifacts],
      downloadBase: downloadBase(repo, version),
      out: updates,
      notes: updateNotes(section, page),
      pubDate: new Date().toISOString(),
      existingBeta: null,
      publicKey: key.publicKey,
    });
    const notes = path.join(dir, "notes.md");
    writeFileSync(notes, githubNotes(section, repo, version));

    /** @type {import("./lib/release.mjs").ReleaseRecord} */
    const record = { version, tag, channel, commit: null, artifacts, notes, updates };
    if (o.dryRun) {
      restore();
      console.log(`\nDry run: built into ${dir}; nothing committed, the version files restored.`);
      return;
    }
    run("git", ["add", "--", CHANGELOG, "Cargo.lock", ...changed], { shell: false });
    run("git", ["commit", "-m", `chore(release): release ${version}`], { shell: false });
    record.commit = git(["rev-parse", "HEAD"]);
    writeFileSync(path.join(dir, RECORD_FILE), `${JSON.stringify(record, null, 2)}\n`);
    console.log(`\nRelease ${version} committed (${record.commit.slice(0, 8)}); files in ${dir}.`);
    console.log("Try the installer, then publish it with: npm run release:publish");
  } catch (error) {
    if (!git(["log", "-1", "--format=%s"]).endsWith(`release ${version}`)) restore();
    throw error;
  }
}

/**
 * The options of `publish`.
 *
 * @param {string[]} argv
 */
function publishArgs(argv) {
  const o = { version: "", yes: false, remote: "github" };
  forEachArg(argv, (arg, { value }) => {
    if (arg === "--version") o.version = value();
    else if (arg === "--yes") o.yes = true;
    else if (arg === "--remote") o.remote = value();
    else throw new Error(`unknown option ${arg}`);
  });
  return o;
}

/**
 * The record of `version`, or of the newest prepared release.
 *
 * @param {string} version
 */
function releaseRecord(version) {
  let chosen = version;
  if (!chosen) {
    const root = path.join(buildBase(process.env, repoRoot), "release");
    const prepared = existsSync(root)
      ? readdirSync(root).filter((v) => existsSync(path.join(root, v, RECORD_FILE)))
      : [];
    chosen = prepared.sort(compareVersions).at(-1) ?? "";
    if (!chosen) throw new Error("no prepared release; run npm run release:prepare first");
  }
  const file = path.join(releaseDir(chosen), RECORD_FILE);
  if (!existsSync(file)) throw new Error(`no prepared release ${chosen} (${file})`);
  const record = /** @type {import("./lib/release.mjs").ReleaseRecord} */ (readJson(file));
  if (!record.commit) throw new Error(`release ${chosen} was a dry run`);
  return /** @type {import("./lib/release.mjs").ReleaseRecord & { commit: string }} */ (record);
}

/**
 * Whether a command succeeds (its output discarded).
 *
 * @param {string} command
 * @param {string[]} args
 */
function succeeds(command, args) {
  try {
    output(command, args, { shell: false });
    return true;
  } catch {
    return false;
  }
}

/**
 * The version of a channel's published manifest, or null.
 *
 * @param {string} slug
 * @param {string} channel
 * @param {string} scratch
 */
function publishedVersion(slug, channel, scratch) {
  const { tag, name } = channelAsset(channel);
  const file = path.join(scratch, channel, name);
  mkdirSync(path.dirname(file), { recursive: true });
  rmSync(file, { force: true });
  const args = [
    "release",
    "download",
    tag,
    "--repo",
    slug,
    "--pattern",
    name,
    "--dir",
    path.dirname(file),
  ];
  if (!succeeds("gh", args)) return null;
  return /** @type {{ version: string }} */ (readJson(file)).version;
}

/** @param {string[]} argv */
async function publish(argv) {
  assertWorkFolder("npm run release:publish");
  const o = publishArgs(argv);
  const record = releaseRecord(o.version);
  const { version, tag, commit } = record;
  const repo = repository();
  const slug = githubSlug(repo);
  const branch = git(["rev-parse", "--abbrev-ref", "HEAD"]);
  if (!succeeds("git", ["merge-base", "--is-ancestor", commit, "HEAD"])) {
    throw new Error(`the release commit ${commit.slice(0, 8)} is not on ${branch}`);
  }
  if (!succeeds("gh", ["auth", "status"]))
    throw new Error("the GitHub CLI is not signed in (gh auth login)");

  console.log(
    `Publish Nail ${version}: tag ${tag} at ${commit.slice(0, 8)}, push ${branch}, GitHub release on ${slug}.`,
  );
  if (!o.yes && !(await confirm("Go on? [y/N] "))) {
    console.error("Not published (answer y, or pass --yes where nothing can be typed).");
    return 1;
  }

  // 1. The tag (an existing one must name the release commit).
  const tagged = succeeds("git", ["rev-parse", "--verify", "--quiet", `refs/tags/${tag}`])
    ? git(["rev-parse", `${tag}^{commit}`])
    : null;
  if (tagged && tagged !== commit) throw new Error(`${tag} exists and names ${tagged.slice(0, 8)}`);
  if (!tagged)
    run("git", ["tag", "--annotate", tag, "--message", `Nail ${version}`, commit], {
      shell: false,
    });

  // 2. The pushes: origin, then the public remote (from the master, if
  // there is one, ADR 0205).
  const pushes = publishPushes({
    branch,
    tag,
    remote: o.remote,
    origin: originOf(repoRoot),
    master: o.remote === "origin" ? undefined : masterOf(repoRoot),
  });
  for (const args of pushes) run("git", args, { shell: false });

  // 3. The GitHub release with every built file.
  const files = readdirSync(record.artifacts).map((f) => path.join(record.artifacts, f));
  const pre = record.channel === "stable" ? ["--latest"] : ["--prerelease", "--latest=false"];
  if (succeeds("gh", ["release", "view", tag, "--repo", slug])) {
    run("gh", ["release", "upload", tag, ...files, "--repo", slug, "--clobber"], { shell: false });
  } else {
    run(
      "gh",
      [
        "release",
        "create",
        tag,
        ...files,
        "--repo",
        slug,
        "--verify-tag",
        "--title",
        `Nail ${version}`,
        "--notes-file",
        record.notes,
        ...pre,
      ],
      { shell: false },
    );
  }

  // 4. The channels' manifests on their rolling releases.
  const scratch = path.join(releaseDir(version), "published");
  for (const channel of CHANNELS) {
    const manifest = path.join(record.updates, channel, MANIFEST_FILE);
    if (!existsSync(manifest)) continue;
    const published = publishedVersion(slug, channel, scratch);
    if (published && compareVersions(published, version) > 0) {
      console.log(`${channel}: keeps ${published} (newer than ${version})`);
      continue;
    }
    const { tag: channelTag } = channelAsset(channel);
    if (!succeeds("gh", ["release", "view", channelTag, "--repo", slug])) {
      run(
        "gh",
        [
          "release",
          "create",
          channelTag,
          "--repo",
          slug,
          "--target",
          commit,
          "--title",
          `Update channel: ${channel}`,
          "--notes",
          `Nail's in-app updates of the ${channel} channel read ${MANIFEST_FILE} from here. Download Nail from the numbered releases.`,
          "--prerelease",
          "--latest=false",
        ],
        { shell: false },
      );
    }
    run("gh", ["release", "upload", channelTag, manifest, "--repo", slug, "--clobber"], {
      shell: false,
    });
  }
  console.log(`\nPublished: ${releasePage(repo, version)}`);
  return 0;
}

/** @param {string[]} argv */
async function main(argv) {
  const [command, ...rest] = argv;
  if (command === "prepare") {
    prepare(rest);
    return 0;
  }
  if (command === "publish") return publish(rest);
  throw new Error("usage: node scripts/release.mjs prepare|publish [options]");
}

await runMain(import.meta.url, main);
