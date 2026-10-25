/**
 * Releases (ADR 0203): the facts and texts `scripts/release.mjs` derives
 * from the repository. Pure helpers; tests in scripts/test/release.test.mjs.
 *
 * - The user-facing notes are the version's section of
 *   `userDocs/en/release-notes.md` (`## … (\`X.Y.Z\`)`): the GitHub release
 *   shows it with links into the tagged sources, the update dialog its
 *   introduction as plain text with a link to the release.
 * - `CHANGELOG.md` (Keep a Changelog) gets its `[Unreleased]` entries
 *   under the version and date.
 * - Each channel is a rolling GitHub release tagged with the channel's
 *   name holding that channel's `latest.json`, so the app's update base is
 *   `<repository>/releases/download` (`<base>/<channel>/latest.json`).
 */
import { CHANNELS, MANIFEST_FILE, parseVersion } from "./update-manifest.mjs";

/** The user-facing release notes. */
export const RELEASE_NOTES = "userDocs/en/release-notes.md";

/** The developer-facing changelog. */
export const CHANGELOG = "CHANGELOG.md";

/** The public half of the release update key. */
export const RELEASE_PUBKEY = "packaging/updater/release.pub";

/** The release record `release:prepare` writes and `release:publish` reads. */
export const RECORD_FILE = "release.json";

/**
 * What `release:prepare` hands to `release:publish`.
 *
 * @typedef {object} ReleaseRecord
 * @property {string} version
 * @property {string} tag
 * @property {string} channel
 * @property {string | null} commit the release commit (null for a dry run)
 * @property {string} artifacts the folder with the files of the GitHub release
 * @property {string} notes the GitHub release's text (Markdown file)
 * @property {string} updates the folder with `<channel>/latest.json`
 */

/** @param {string} version */
export function releaseTag(version) {
  return `v${version}`;
}

/**
 * `beta` for a pre-release (`0.9.0-beta.1`), else `stable`.
 *
 * @param {string} version
 */
export function channelOf(version) {
  const channel = parseVersion(version).pre.length ? "beta" : "stable";
  if (!CHANNELS.includes(channel)) throw new Error(`no update channel ${channel}`);
  return channel;
}

/**
 * `owner/name` of a GitHub repository URL (for `gh --repo`).
 *
 * @param {string} repository `https://github.com/<owner>/<name>`
 */
export function githubSlug(repository) {
  const match = /^https:\/\/github\.com\/([^/]+\/[^/]+?)(?:\.git)?\/?$/.exec(repository);
  if (!match?.[1]) throw new Error(`not a GitHub repository URL: ${repository}`);
  return match[1];
}

/**
 * The update base built into the app: the channel releases' downloads.
 *
 * @param {string} repository
 */
export function updateBase(repository) {
  return `${repository}/releases/download`;
}

/**
 * Where a channel's manifest is published: the asset `latest.json` of
 * the release tagged with the channel's name.
 *
 * @param {string} channel
 */
export function channelAsset(channel) {
  return { tag: channel, name: MANIFEST_FILE };
}

/**
 * The downloads of a version's release.
 *
 * @param {string} repository
 * @param {string} version
 */
export function downloadBase(repository, version) {
  return `${updateBase(repository)}/${releaseTag(version)}`;
}

/**
 * The version's release page.
 *
 * @param {string} repository
 * @param {string} version
 */
export function releasePage(repository, version) {
  return `${repository}/releases/tag/${releaseTag(version)}`;
}

/** @param {string} text */
function escapeRegExp(text) {
  return text.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * The version's section of the release notes (without its heading), or
 * `null` when there is none.
 *
 * @param {string} markdown `userDocs/en/release-notes.md`
 * @param {string} version
 */
export function releaseSection(markdown, version) {
  const heading = new RegExp(`^## .*\\(\`${escapeRegExp(version)}\`\\)[ \\t]*$`, "m");
  const match = heading.exec(markdown);
  if (!match) return null;
  const rest = markdown.slice(match.index + match[0].length);
  const next = rest.search(/^## /m);
  return (next < 0 ? rest : rest.slice(0, next)).trim();
}

/**
 * `markdown` with relative links (`[a](other.md)`) pointing below `base`.
 *
 * @param {string} markdown
 * @param {string} base
 */
export function absoluteLinks(markdown, base) {
  return markdown.replace(/\]\((?![a-z][a-z0-9+.-]*:|#|\/)([^)\s]+)\)/gi, `](${base}/$1)`);
}

/**
 * `markdown` with every link reduced to its text.
 *
 * @param {string} markdown
 */
export function plainLinks(markdown) {
  return markdown.replace(/\[([^\]]+)\]\([^)]*\)/g, "$1");
}

/**
 * `markdown` with the lines of each paragraph and list item joined: a
 * GitHub release shows every line break. Code blocks stay as they are.
 *
 * @param {string} markdown
 */
export function unwrapped(markdown) {
  /** @type {string[]} */
  const lines = [];
  let fenced = false;
  for (const line of markdown.split("\n")) {
    const text = line.trim();
    if (text.startsWith("```")) fenced = !fenced;
    const previous = lines.at(-1);
    const starts = /^(?:#|[-*+] |\d+\. |```|>|\|)/.test(text);
    if (!fenced && text && !starts && previous?.trim() && !/^(?:#|```)/.test(previous.trim())) {
      lines[lines.length - 1] = `${previous} ${text}`;
    } else {
      lines.push(line);
    }
  }
  return lines.join("\n");
}

/**
 * The GitHub release's text: the section, its lines unwrapped and its
 * links into the tagged sources.
 *
 * @param {string} section
 * @param {string} repository
 * @param {string} version
 */
export function githubNotes(section, repository, version) {
  const base = `${repository}/blob/${releaseTag(version)}/${RELEASE_NOTES.replace(/\/[^/]+$/, "")}`;
  return `${absoluteLinks(unwrapped(section), base)}\n`;
}

/**
 * The update dialog's text: the section's introduction (before its first
 * subheading) as plain text, and where the rest is.
 *
 * @param {string} section
 * @param {string} page the release page
 */
export function updateNotes(section, page) {
  const intro = section.split(/^### /m)[0]?.trim() ?? "";
  const text = plainLinks(intro).replace(/\s*\n\s*/g, " ");
  return `${text}\n\nAll changes: ${page}`.trim();
}

/**
 * `CHANGELOG.md` with its `[Unreleased]` entries under `version` and
 * `date`, and a link to the release; unchanged when the version is there.
 *
 * @param {string} changelog
 * @param {{ version: string, date: string, page: string }} release
 */
export function releasedChangelog(changelog, { version, date, page }) {
  if (changelog.includes(`## [${version}]`)) return changelog;
  const unreleased = "## [Unreleased]\n";
  if (!changelog.includes(unreleased)) throw new Error(`${CHANGELOG} has no ## [Unreleased]`);
  const moved = changelog.replace(unreleased, `${unreleased}\n## [${version}] — ${date}\n`);
  const link = `[${version}]: ${page}\n`;
  const firstLink = moved.search(/^\[[^\]]+\]: /m);
  return firstLink < 0
    ? `${moved.trimEnd()}\n\n${link}`
    : moved.slice(0, firstLink) + link + moved.slice(firstLink);
}
