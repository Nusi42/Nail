/**
 * What the pre-push hook runs for a push (ADR 0205): the decision in one
 * function, {@link pushPlan}, from the pushing checkout's role and the
 * remote's URL. scripts/pre-push.mjs runs the plan; tests in
 * scripts/test/push-plan.test.mjs (every combination) and
 * scripts/test/hooks.test.mjs (the hook itself, through git).
 *
 * - The master (`NAIL_CHECKOUT_ROLE=master`) is the only checkout that
 *   publishes: it pushes `main` (and release tags) to a network remote and
 *   runs only checks that build nothing (the private-data guard on the
 *   pushed commits and on every tracked file, no merge commits (ADR 0208),
 *   the dependency licences).
 *   `main` only moves to a commit that passed the full check in a work
 *   folder, so the full check is not repeated.
 * - A work folder (its `origin` is a master checkout) pushes only to a
 *   local path: the guard on the pushed commits, no merge commits, then
 *   `npm run check`.
 * - A plain clone (no master) keeps the full checks for every push.
 * - Every other combination is refused.
 */
import { execFileSync } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { DENY_ARGS, INSTALL, PRIVATE_CHECK } from "./check-steps.mjs";
import { releaseTag } from "./release.mjs";
import { parseVersion } from "./update-manifest.mjs";
import { isMaster, originOf } from "./work-folder.mjs";

/** The remote the master publishes to. */
export const PUBLIC_REMOTE = "github";

/** The branch the master publishes. */
export const PUBLISH_BRANCH = "main";

/**
 * The command that publishes `main` from the master.
 *
 * @param {string} master the master checkout's path
 * @returns {string}
 */
export function publishCommand(master) {
  return `git -C "${master}" push ${PUBLIC_REMOTE} ${PUBLISH_BRANCH}`;
}

/**
 * The `git` calls that publish a release's branch and tag
 * (`npm run release:publish`, ADR 0203). With a master: push both to it,
 * fast-forward its `main`, and let the master push `main` and the tag to the
 * public remote. Without one (a plain clone): push from here.
 *
 * @param {{ branch: string, tag: string, remote: string, origin?: string, master?: string }} o
 *   `origin`: this checkout's origin, if any; `master`: the master's path, if any
 * @returns {string[][]}
 */
export function publishPushes({ branch, tag, remote, origin, master }) {
  /** @type {string[][]} */
  const calls = [];
  if (origin !== undefined && remote !== "origin") calls.push(["push", "origin", branch, tag]);
  if (master === undefined) {
    calls.push(["push", remote, `${branch}:${PUBLISH_BRANCH}`, tag]);
  } else {
    calls.push(["-C", master, "merge", "--ff-only", branch]);
    calls.push(["-C", master, "push", remote, PUBLISH_BRANCH, tag]);
  }
  return calls;
}

/** URLs that name a path on this machine; everything else is a network remote. */
const LOCAL_URL = [
  /^[A-Za-z]:[\\/]/, // C:\… or C:/…
  /^\\\\/, // \\host\share
  /^\//, // /…
  /^\.\.?(?:[\\/]|$)/, // ./x, ../x
  /^file:\/\//i,
];

/**
 * Whether a remote URL is a local path (`C:\…`, `/…`, `../x`,
 * `\\host\share`, `file://…`); `https://…`, `ssh://…`, `git@host:…` and
 * every other form are network remotes.
 *
 * @param {string} url
 * @returns {boolean}
 */
export function isLocalUrl(url) {
  return LOCAL_URL.some((re) => re.test(url.trim()));
}

/**
 * The directory a local remote URL names, resolved against `root`.
 *
 * @param {string} root the checkout the URL is configured in
 * @param {string} url a URL for which {@link isLocalUrl} holds
 * @returns {string}
 */
export function localPath(root, url) {
  const trimmed = url.trim();
  return /^file:\/\//i.test(trimmed) ? fileURLToPath(trimmed) : path.resolve(root, trimmed);
}

/** @typedef {"master" | "work" | "plain"} CheckoutRole */

/**
 * The master checkout a work folder pushes to (its local `origin`, marked as
 * the master), or `undefined` when the checkout has none.
 *
 * @param {string} root
 * @returns {string | undefined}
 */
export function masterOf(root) {
  const origin = originOf(root);
  if (origin === undefined || !isLocalUrl(origin)) return undefined;
  const dir = localPath(root, origin);
  // Only the origin's own .env.local counts, not this process's environment.
  return isMaster(dir, {}) ? dir : undefined;
}

/**
 * The role of the checkout at `root`: the master, a work folder of a master,
 * or a plain clone.
 *
 * @param {string} root
 * @param {NodeJS.ProcessEnv} [env]
 * @returns {CheckoutRole}
 */
export function checkoutRole(root, env = process.env) {
  if (isMaster(root, env)) return "master";
  return masterOf(root) === undefined ? "plain" : "work";
}

/** @typedef {{ localRef: string, localSha: string, remoteRef: string, remoteSha: string }} PushRef */

/**
 * Parses the ref lines git's pre-push hook reads on stdin
 * (`<local ref> <local sha> <remote ref> <remote sha>`).
 *
 * @param {string} text
 * @returns {PushRef[]}
 */
export function parsePushLines(text) {
  return text
    .split(/\r?\n/)
    .map((line) => line.trim().split(/\s+/))
    .filter((parts) => parts.length >= 4)
    .map(([localRef = "", localSha = "", remoteRef = "", remoteSha = ""]) => ({
      localRef,
      localSha,
      remoteRef,
      remoteSha,
    }));
}

/** The object id git's pre-push hook reports for "no object" (a deletion, a new ref). */
const NO_OBJECT = /^0+$/;

/**
 * Whether the repository at `root` has the commit `sha`.
 *
 * @param {string} root
 * @param {string} sha
 * @returns {boolean}
 */
export function hasCommit(root, sha) {
  try {
    execFileSync("git", ["cat-file", "-e", `${sha}^{commit}`], { cwd: root, stdio: "ignore" });
    return true;
  } catch {
    return false;
  }
}

/**
 * The revision arguments (`git log` / `git rev-list`) that select the
 * commits a ref line pushes: those on no remote-tracking branch and not
 * under the remote's old value. `undefined` for a deletion.
 *
 * @param {PushRef} ref
 * @param {(sha: string) => boolean} known whether the repository has that commit
 * @returns {string[] | undefined}
 */
export function pushedRange(ref, known) {
  if (NO_OBJECT.test(ref.localSha)) return undefined;
  const range = [ref.localSha, "--not", "--remotes"];
  if (!NO_OBJECT.test(ref.remoteSha) && known(ref.remoteSha)) range.push(ref.remoteSha);
  return range;
}

/**
 * Whether `ref` is a release tag (`refs/tags/v<version>`, ADR 0203).
 *
 * @param {string} ref
 * @returns {boolean}
 */
export function isReleaseTagRef(ref) {
  const name = ref.replace(/^refs\/tags\//, "");
  if (name === ref || !name.startsWith(releaseTag(""))) return false;
  try {
    parseVersion(name.slice(releaseTag("").length));
    return true;
  } catch {
    return false;
  }
}

/** Whether the master may push this ref: `main` to `main`, or a release tag to itself. */
const publishable = (/** @type {PushRef} */ ref) =>
  (ref.localRef === `refs/heads/${PUBLISH_BRANCH}` &&
    ref.remoteRef === `refs/heads/${PUBLISH_BRANCH}`) ||
  (ref.localRef === ref.remoteRef && isReleaseTagRef(ref.localRef));

/**
 * @typedef {object} PushStep
 * @property {string} id
 * @property {string} title
 * @property {string} tool `node`, or an executable found on PATH
 * @property {string[]} args
 * @property {boolean} [stdin] gets git's ref lines on stdin
 * @property {string} install
 */

/** @type {PushStep} */
const PRIVATE_PUSHED = {
  id: "private-push",
  title: "No private data in the pushed commits (messages, identities, added lines)",
  tool: "node",
  args: [...PRIVATE_CHECK, "--push"],
  stdin: true,
  install: INSTALL.npm,
};

/** @type {PushStep} */
const LINEAR = {
  id: "linear",
  title: "No merge commits in the pushed history (ADR 0208)",
  tool: "node",
  args: ["scripts/check-linear.mjs"],
  stdin: true,
  install: INSTALL.npm,
};

/** @type {PushStep} */
const PRIVATE_TREE = {
  id: "private",
  title: "No private data in the tracked files",
  tool: "node",
  args: PRIVATE_CHECK,
  install: INSTALL.npm,
};

/** @type {PushStep} */
const LICENSES = {
  id: "licenses",
  title: "Dependency licences (cargo deny check licenses)",
  tool: "cargo-deny",
  args: [...DENY_ARGS, "licenses"],
  install: INSTALL.deny,
};

/** @type {PushStep} */
const FULL_CHECK = {
  id: "check",
  title: "npm run check",
  tool: "node",
  args: ["scripts/check.mjs"],
  install: INSTALL.npm,
};

/**
 * What a push runs, or why it is refused.
 *
 * @param {{ role: CheckoutRole, url: string, refs: PushRef[], master?: string }} push
 *   `master`: the master's path, named in a work folder's refusal
 * @returns {{ steps: PushStep[] } | { refusal: string }}
 */
export function pushPlan({ role, url, refs, master = "<master>" }) {
  const local = isLocalUrl(url);
  if (role === "plain") return { steps: [PRIVATE_PUSHED, FULL_CHECK] };
  if (role === "work") {
    if (local) return { steps: [PRIVATE_PUSHED, LINEAR, FULL_CHECK] };
    return {
      refusal:
        `a work folder pushes only to the master, not to ${url}.\n` +
        `Push the branch to origin, fast-forward the master, then publish from it:\n` +
        `  ${publishCommand(master)}`,
    };
  }
  if (local) {
    return {
      refusal:
        `the master pushes only to the public remote, not to ${url}; ` +
        "work folders fetch from it instead.",
    };
  }
  const other = refs.filter((ref) => !publishable(ref));
  if (other.length > 0) {
    return {
      refusal:
        `the master publishes only ${PUBLISH_BRANCH} (and release tags), not ` +
        `${other.map((r) => `${r.localRef} -> ${r.remoteRef}`).join(", ")}.`,
    };
  }
  return { steps: [PRIVATE_PUSHED, LINEAR, PRIVATE_TREE, LICENSES] };
}
