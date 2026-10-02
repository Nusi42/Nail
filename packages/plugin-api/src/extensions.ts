/**
 * Helpers for hooks and extensions (M4a, ADR 0079): builders for the
 * answers of hooks and preview renderers, and quick checks that mirror
 * the host's validation (`problems()` / `image_ok()` in
 * `crates/nail-api/src/plugins/{hooks,extensions}.rs`). The host stays
 * authoritative: an answer it finds invalid is skipped and counted as a
 * failed call.
 */
import type {
  PluginArrivalResult,
  PluginBadgeSpec,
  PluginBannerButton,
  PluginBannerSpec,
  PluginBeforeSendResult,
  PluginContentAddition,
  PluginMarkdownBlockResult,
  PluginNotificationLevel,
  PluginOpenResult,
  PluginPreview,
} from "./generated";
import type { PluginWriteResult } from "./api";
import { toBase64, toBytes } from "./bytes";
import { toNodes, type UiChild } from "./jsx-runtime";
import { checkTree } from "./tree";

/** Longest veto reason, banner text or badge tooltip (characters). */
export const MAX_HOOK_TEXT = 500;
/** Longest badge text (characters). */
export const MAX_BADGE_TEXT = 32;
/** Most banners and most badges one open hook call may answer. */
export const MAX_OPEN_ITEMS = 3;
/** Most buttons of one banner. */
export const MAX_BANNER_BUTTONS = 3;
/** Most actions one arrival hook call may answer. */
export const MAX_ARRIVAL_ACTIONS = 20;
/** Most headers one before-send call may add. */
export const MAX_ADDED_HEADERS = 10;
/** Longest header name (bytes). */
export const MAX_HEADER_NAME = 76;
/** Longest added header value (bytes). */
export const MAX_HEADER_VALUE = 998;
/** Largest content one before-send call may add (UTF-8 bytes of text + HTML). */
export const MAX_ADDED_CONTENT = 64 * 1024;
/** Header names plug-ins may never add (case-insensitive prefix). */
export const RESERVED_HEADER_PREFIX = "X-Nail-";
/** Image types a preview may answer (no SVG: it can carry script). */
export const PREVIEW_IMAGE_TYPES = ["image/png", "image/jpeg", "image/gif", "image/webp"] as const;
/** Largest preview image (bytes, decoded). */
export const MAX_PREVIEW_IMAGE = 5 * 1024 * 1024;
/** Largest attachment a preview renderer gets (bytes). */
export const MAX_PREVIEW_INPUT = 10 * 1024 * 1024;
/** Largest HTML a Markdown block may render to (bytes). */
export const MAX_BLOCK_HTML = 256 * 1024;
/** Default budgets of the hooks (ms). */
export const HOOK_TIMEOUT_MS = { arrival: 2_000, beforeSend: 3_000, open: 1_000 } as const;
/** Shortest and longest budget a manifest may ask for (ms). */
export const MIN_HOOK_TIMEOUT_MS = 50;
export const MAX_HOOK_TIMEOUT_MS = 10_000;
/** Wall-clock limit of one background job run (ms). */
export const JOB_WALL_CLOCK_MS = 60_000;

const chars = (text: string) => [...text].length;
const utf8Length = (text: string) => {
  let n = 0;
  for (const ch of text) {
    const cp = ch.codePointAt(0) ?? 0;
    n += cp < 0x80 ? 1 : cp < 0x800 ? 2 : cp < 0x10000 ? 3 : 4;
  }
  return n;
};

/** Kebab-case name rule of ids (banners, buttons, contributions). */
export function isContributionName(name: string): boolean {
  return (
    name.length > 0 &&
    name.length <= 64 &&
    /^[a-z][a-z0-9-]*$/.test(name) &&
    !name.endsWith("-") &&
    !name.includes("--")
  );
}

/** Whether a plug-in may add a header of this name: `X-…`, not `X-Nail-…`. */
export function isPluginHeaderName(name: string): boolean {
  const lower = name.toLowerCase();
  return (
    name.length > 2 &&
    utf8Length(name) <= MAX_HEADER_NAME &&
    lower.startsWith("x-") &&
    !lower.startsWith(RESERVED_HEADER_PREFIX.toLowerCase()) &&
    /^[\x21-\x39\x3b-\x7e]+$/.test(name)
  );
}

/** Whether `value` is one header line (no CR, LF or NUL; at most 998 bytes). */
export function isHeaderValue(value: string): boolean {
  return utf8Length(value) <= MAX_HEADER_VALUE && !/[\r\n\0]/.test(value);
}

/** Problems with an arrival hook's answer (empty = valid). */
export function arrivalProblems(result: PluginArrivalResult | null | undefined): string[] {
  const out: string[] = [];
  const actions = result?.actions ?? [];
  if (actions.length > MAX_ARRIVAL_ACTIONS) out.push("too many actions");
  const types = new Set(["addTag", "removeTag", "setFlagged", "setRead", "move", "archive"]);
  if (actions.some((a) => !types.has((a as { type?: string }).type ?? ""))) {
    out.push("unknown action");
  }
  return out;
}

/** Problems with a before-send hook's answer (empty = valid). */
export function beforeSendProblems(result: PluginBeforeSendResult | null | undefined): string[] {
  const out: string[] = [];
  if (!result) return out;
  const veto = result.veto;
  if (veto && (!veto.reason.trim() || chars(veto.reason) > MAX_HOOK_TEXT)) {
    out.push("veto reason empty or too long");
  }
  const headers = result.headers ?? [];
  if (headers.length > MAX_ADDED_HEADERS) out.push("too many headers");
  if (headers.some((h) => !isPluginHeaderName(h.name) || !isHeaderValue(h.value))) {
    out.push("invalid header");
  }
  const size = (result.addContent ?? []).reduce(
    (n, c) => n + utf8Length(c.text) + (c.html ? utf8Length(c.html) : 0),
    0,
  );
  if (size > MAX_ADDED_CONTENT) out.push("added content too large");
  return out;
}

/** Problems with an open hook's answer (empty = valid). */
export function openProblems(result: PluginOpenResult | null | undefined): string[] {
  const out: string[] = [];
  if (!result) return out;
  const banners = result.banners ?? [];
  const badges = result.badges ?? [];
  if (banners.length > MAX_OPEN_ITEMS || badges.length > MAX_OPEN_ITEMS) {
    out.push("too many banners or badges");
  }
  if (
    banners.some(
      (b) =>
        !isContributionName(b.id) ||
        !b.text.trim() ||
        chars(b.text) > MAX_HOOK_TEXT ||
        (b.buttons ?? []).length > MAX_BANNER_BUTTONS ||
        (b.buttons ?? []).some((x) => !isContributionName(x.id) || !x.label.trim()),
    )
  ) {
    out.push("invalid banner");
  }
  if (
    badges.some(
      (b) =>
        !b.text.trim() ||
        chars(b.text) > MAX_BADGE_TEXT ||
        (b.tooltip !== undefined && chars(b.tooltip) > MAX_HOOK_TEXT),
    )
  ) {
    out.push("invalid badge");
  }
  return out;
}

/** Problems with a preview renderer's answer (empty = valid). */
export function previewProblems(preview: PluginPreview | null | undefined): string[] {
  if (!preview || typeof preview !== "object") return ["not a preview"];
  if (preview.kind === "tree") {
    if (!Array.isArray(preview.tree)) return ["not a preview"];
    return checkTree(preview.tree, "tree").map((i) => `${i.path}: ${i.message}`);
  }
  if (preview.kind === "image") {
    const out: string[] = [];
    if (!(PREVIEW_IMAGE_TYPES as readonly string[]).includes(preview.mimeType)) {
      out.push("image type not allowed");
    }
    if (Math.floor(preview.data.length / 4) * 3 > MAX_PREVIEW_IMAGE) out.push("image too large");
    return out;
  }
  return ["not a preview"];
}

/** Problems with a Markdown block renderer's answer (empty = valid). */
export function markdownBlockProblems(
  result: PluginMarkdownBlockResult | null | undefined,
): string[] {
  if (!result || typeof result.html !== "string" || typeof result.text !== "string") {
    return ["not a block result"];
  }
  return utf8Length(result.html) > MAX_BLOCK_HTML ? ["html too large"] : [];
}

/** A before-send answer that stops sending with `reason` (the user can "Send anyway"). */
export function veto(reason: string): PluginBeforeSendResult {
  return { veto: { reason } };
}

/** A before-send answer that adds content at the top or bottom. */
export function addContent(
  text: string,
  options: { position?: PluginContentAddition["position"]; html?: string } = {},
): PluginBeforeSendResult {
  const addition: PluginContentAddition = { text };
  if (options.position) addition.position = options.position;
  if (options.html !== undefined) addition.html = options.html;
  return { addContent: [addition] };
}

/** Merges before-send answers (vetoes: the first wins). */
export function mergeBeforeSend(
  ...results: (PluginBeforeSendResult | null | undefined)[]
): PluginBeforeSendResult {
  const out: PluginBeforeSendResult = {};
  for (const r of results) {
    if (!r) continue;
    if (r.veto && !out.veto) out.veto = r.veto;
    if (r.headers?.length) out.headers = [...(out.headers ?? []), ...r.headers];
    if (r.addContent?.length) out.addContent = [...(out.addContent ?? []), ...r.addContent];
  }
  return out;
}

/** A banner for an open hook's answer. */
export function banner(
  id: string,
  text: string,
  options: { level?: PluginNotificationLevel; buttons?: PluginBannerButton[] } = {},
): PluginBannerSpec {
  const spec: PluginBannerSpec = { id, text };
  if (options.level) spec.level = options.level;
  if (options.buttons?.length) spec.buttons = options.buttons;
  return spec;
}

/** A badge for an open hook's answer. */
export function badge(
  text: string,
  options: { level?: PluginNotificationLevel; icon?: string; tooltip?: string } = {},
): PluginBadgeSpec {
  const spec: PluginBadgeSpec = { text };
  if (options.level) spec.level = options.level;
  if (options.icon) spec.icon = options.icon;
  if (options.tooltip) spec.tooltip = options.tooltip;
  return spec;
}

/** A preview drawn as a component tree; `content` may be JSX. */
export function previewTree(content: UiChild): PluginPreview {
  return { kind: "tree", tree: toNodes(content) };
}

/** A preview image (PNG, JPEG, GIF or WebP; bytes or base64). */
export function previewImage(
  mimeType: (typeof PREVIEW_IMAGE_TYPES)[number],
  data: Uint8Array | ArrayBuffer | string,
  alt?: string,
): PluginPreview {
  const base64 = typeof data === "string" ? data : toBase64(toBytes(data));
  const preview: PluginPreview = { kind: "image", mimeType, data: base64 };
  if (alt !== undefined) preview.alt = alt;
  return preview;
}

/** Whether `value` is already a {@link PluginPreview}. */
export function isPreview(value: unknown): value is PluginPreview {
  if (typeof value !== "object" || value === null || Array.isArray(value)) return false;
  const kind = (value as { kind?: unknown }).kind;
  return (
    (kind === "tree" && Array.isArray((value as { tree?: unknown }).tree)) ||
    (kind === "image" && typeof (value as { data?: unknown }).data === "string")
  );
}

/** Whether a write was done (not denied, expired or vetoed). */
export function isDone(
  result: PluginWriteResult,
): result is Extract<PluginWriteResult, { status: "done" }> {
  return result.status === "done";
}

/** The first id a done write created (the new task, draft, event, contact). */
export function createdId(result: PluginWriteResult): string | undefined {
  return result.status === "done" ? result.created[0] : undefined;
}
