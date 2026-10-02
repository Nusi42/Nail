/**
 * The M4b desktop contract as the UI sees it (mirrors `nail_api::desktop` /
 * `nail_api::mapi`; ADR 0087): lists of the enum values and the Fluent ids
 * the contract names (texts in `locales/en/core-desktop.ftl`).
 */
import type {
  DefaultAppState,
  DefaultAppTarget,
  InstallKind,
  InstallScope,
  MapiResult,
  UpdateChannel,
  UpdateDisabledReason,
  UpdateFailure,
  UpdateInfo,
  UpdateState,
} from "../../api/generated";

export const INSTALL_KINDS = [
  "nsis",
  "msi",
  "portable",
  "development",
] as const satisfies readonly InstallKind[];
export const INSTALL_SCOPES = [
  "per-user",
  "per-machine",
] as const satisfies readonly InstallScope[];
export const UPDATE_CHANNELS = ["stable", "beta"] as const satisfies readonly UpdateChannel[];
export const UPDATE_DISABLED_REASONS = [
  "msi-managed",
  "policy",
  "portable",
  "development",
  "not-configured",
] as const satisfies readonly UpdateDisabledReason[];
export const UPDATE_FAILURES = [
  "network",
  "manifest",
  "signature",
  "download",
  "install",
] as const satisfies readonly UpdateFailure[];
/** In the order the UI lists them (`DefaultAppTarget::ALL`). */
export const DEFAULT_APP_TARGETS = [
  "mailto",
  "webcal",
  "webcals",
  "eml",
  "ics",
  "vcf",
] as const satisfies readonly DefaultAppTarget[];
export const DEFAULT_APP_STATES = [
  "default",
  "other",
  "unknown",
] as const satisfies readonly DefaultAppState[];
export const MAPI_RESULTS = [
  "success",
  "user-abort",
  "failure",
  "login-failure",
  "insufficient-memory",
  "access-denied",
  "too-many-files",
  "too-many-recipients",
  "attachment-not-found",
  "attachment-open-failure",
  "unknown-recipient",
  "bad-recip-type",
  "text-too-large",
  "invalid-recips",
  "not-supported",
  "attachment-too-large",
] as const satisfies readonly MapiResult[];

export const installKindMessageId = (kind: InstallKind) => `install-kind-${kind}`;
export const installScopeMessageId = (scope: InstallScope) => `install-scope-${scope}`;
export const channelMessageId = (channel: UpdateChannel) => `update-channel-${channel}`;
export const disabledReasonMessageId = (reason: UpdateDisabledReason) =>
  `update-disabled-${reason}`;
export const failureMessageId = (failure: UpdateFailure) => `update-failure-${failure}`;
export const defaultAppMessageId = (target: DefaultAppTarget) => `default-app-${target}`;
export const defaultAppStateMessageId = (state: DefaultAppState) => `default-app-state-${state}`;
export const mapiResultMessageId = (result: MapiResult) => `mapi-result-${result}`;

/** Whether "Check now" applies (`UpdateState::can_check`). */
export function canCheck(state: UpdateState): boolean {
  return ["idle", "up-to-date", "available", "ready-to-install", "failed"].includes(state.state);
}

/** Whether "Download" applies (`UpdateState::can_download`). */
export function canDownload(state: UpdateState): boolean {
  return state.state === "available";
}

/** Whether "Restart to install" applies (`UpdateState::can_install`). */
export function canInstall(state: UpdateState): boolean {
  return state.state === "ready-to-install";
}

/** The release an update state concerns, if any. */
export function updateOf(state: UpdateState): UpdateInfo | null {
  switch (state.state) {
    case "available":
    case "downloading":
    case "ready-to-install":
    case "installing":
      return state.update;
    case "failed":
      return state.update;
    default:
      return null;
  }
}

/** Whether the updater is working (checking, downloading, installing). */
export function isBusy(state: UpdateState): boolean {
  return ["checking", "downloading", "installing"].includes(state.state);
}

/** Download progress in percent (0–100), `null` when the size is unknown. */
export function progressPercent(state: UpdateState): number | null {
  if (state.state !== "downloading") return null;
  const total = state.total_bytes ?? state.update.size_bytes;
  if (total === null || total <= 0) return null;
  return Math.min(100, Math.max(0, Math.round((state.downloaded_bytes / total) * 100)));
}

/**
 * Compares two semver versions (`1.2.0`, `1.2.0-beta.1`): negative when
 * `a` is older, positive when newer, 0 when equal. Pre-releases sort before
 * their release; build metadata is ignored.
 */
export function compareVersions(a: string, b: string): number {
  const split = (v: string) => {
    const [core = "", pre] = (v.replace(/^v/, "").split("+")[0] ?? "").split(/-(.*)/s);
    return { core: core.split(".").map((n) => Number.parseInt(n, 10) || 0), pre: pre ?? null };
  };
  const x = split(a);
  const y = split(b);
  for (let i = 0; i < Math.max(x.core.length, y.core.length); i++) {
    const d = (x.core[i] ?? 0) - (y.core[i] ?? 0);
    if (d !== 0) return d;
  }
  if (x.pre === y.pre) return 0;
  if (x.pre === null) return 1;
  if (y.pre === null) return -1;
  const xs = x.pre.split(".");
  const ys = y.pre.split(".");
  for (let i = 0; i < Math.max(xs.length, ys.length); i++) {
    const p = xs[i];
    const q = ys[i];
    if (p === undefined) return -1;
    if (q === undefined) return 1;
    const pn = /^\d+$/.test(p) ? Number(p) : null;
    const qn = /^\d+$/.test(q) ? Number(q) : null;
    if (pn !== null && qn !== null) {
      if (pn !== qn) return pn - qn;
    } else if (pn !== null) return -1;
    else if (qn !== null) return 1;
    else if (p !== q) return p < q ? -1 : 1;
  }
  return 0;
}

/** One block of release notes (see {@link parseReleaseNotes}). */
export type NotesBlock =
  | { kind: "heading"; text: string }
  | { kind: "paragraph"; text: string }
  | { kind: "list"; items: string[] };

/**
 * Release notes are untrusted Markdown (ADR 0087): the UI shows them as
 * text only. This reads the few shapes release notes use — headings,
 * bullet lists, paragraphs — and strips inline markers (`**`, `` ` ``,
 * link targets); nothing becomes HTML.
 */
export function parseReleaseNotes(markdown: string): NotesBlock[] {
  const inline = (text: string) =>
    text
      .replace(/!?\[([^\]]*)\]\([^)]*\)/g, "$1")
      .replace(/(\*\*|__|`)/g, "")
      .replace(/(^|\W)[*_]([^*_]+)[*_](?=\W|$)/g, "$1$2")
      .trim();
  const blocks: NotesBlock[] = [];
  let paragraph: string[] = [];
  let list: string[] | null = null;
  const flush = () => {
    if (paragraph.length > 0) blocks.push({ kind: "paragraph", text: inline(paragraph.join(" ")) });
    paragraph = [];
    if (list !== null && list.length > 0) blocks.push({ kind: "list", items: list });
    list = null;
  };
  for (const raw of markdown.replace(/\r\n?/g, "\n").split("\n")) {
    const line = raw.trim();
    const heading = /^#{1,6}\s+(.*)$/.exec(line);
    const item = /^(?:[-*+]|\d+[.)])\s+(.*)$/.exec(line);
    if (line === "") {
      flush();
    } else if (heading) {
      flush();
      blocks.push({ kind: "heading", text: inline(heading[1] ?? "") });
    } else if (item) {
      if (paragraph.length > 0) {
        blocks.push({ kind: "paragraph", text: inline(paragraph.join(" ")) });
        paragraph = [];
      }
      list ??= [];
      list.push(inline(item[1] ?? ""));
    } else if (list !== null && raw.startsWith(" ")) {
      list[list.length - 1] = `${list[list.length - 1] ?? ""} ${inline(line)}`.trim();
    } else {
      if (list !== null) {
        blocks.push({ kind: "list", items: list });
        list = null;
      }
      paragraph.push(line);
    }
  }
  flush();
  return blocks.filter((b) => b.kind === "list" || b.text !== "");
}
