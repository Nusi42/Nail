/**
 * The document rendered inside the sandboxed reader iframe.
 *
 * The iframe has `sandbox=""` (no scripts, opaque origin) and the document
 * carries its own Content-Security-Policy: nothing may be loaded except inline
 * styles, `data:` images/fonts and — for remote content the user allowed —
 * the core's proxy. The core's sanitizer removes remote content or, when
 * allowed, rewrites it to signed `nail-proxy:<token>` URLs (ADR 0038),
 * which this module points at the transport's proxy: `/api/proxy/<token>`
 * in the browser, the `nail-proxy` protocol in the desktop app.
 */
import { convertFileSrc } from "@tauri-apps/api/core";
import type { ResolvedTheme } from "../api/generated";
import { isSafeValue, resolveTokenRefs } from "./compile";

/** `nail_api::remote_content::PROXY_URL_SCHEME`. */
export const PROXY_URL_SCHEME = "nail-proxy:";
/** `nail_api::remote_content::PROXY_PATH`. */
export const PROXY_PATH = "/api/proxy/";

/** CSP of the reader document (nothing remote). */
export const READER_CSP =
  "default-src 'none'; style-src 'unsafe-inline'; img-src data: cid:; font-src data:; form-action 'none'; base-uri 'none'";

/** CSP of the reader document that may load through the proxy at `origin`. */
export function readerCsp(proxyOrigin: string | null): string {
  if (proxyOrigin === null) return READER_CSP;
  return `default-src 'none'; style-src 'unsafe-inline'; img-src data: cid: ${proxyOrigin}; font-src data: ${proxyOrigin}; form-action 'none'; base-uri 'none'`;
}

/**
 * Where `nail-proxy:<token>` points in this window: the desktop app's
 * protocol, or the page's own proxy route.
 */
export function proxyBase(win: Window = window): string | null {
  if ("__TAURI_INTERNALS__" in win) return convertFileSrc("", "nail-proxy");
  const { protocol, origin } = win.location;
  return protocol === "http:" || protocol === "https:" ? `${origin}${PROXY_PATH}` : null;
}

/** The origin of a proxy base URL (for the CSP). */
function originOf(base: string): string | null {
  try {
    return new URL(base).origin;
  } catch {
    return null;
  }
}

function safe(value: string, fallback: string): string {
  return isSafeValue(value) ? value : fallback;
}

/** Reader stylesheet for `mode`, with all token references resolved. */
export function readerCss(theme: ResolvedTheme, mode: string): string {
  const r = (value: string, fallback: string) =>
    safe(resolveTokenRefs(value, theme, mode), fallback);
  const m = theme.message;
  const fg = r("var(--nail-color-fg)", "CanvasText");
  const bg = r("var(--nail-color-bg)", "Canvas");
  const accent = r("var(--nail-color-accent)", "LinkText");
  const dark = mode === "dark" || mode === "high-contrast";
  const rules = [
    `:root { color-scheme: ${dark ? "dark" : "light"}; }`,
    `body { margin: 0; padding: 16px 24px; font-family: ${r(m.font_family, "sans-serif")}; font-size: ${r(m.font_size, "14px")}; line-height: ${r(m.line_height, "1.5")}; color: ${fg}; background: ${bg}; overflow-wrap: anywhere; }`,
    `body > * { max-width: ${r(m.max_width, "none")}; }`,
    `a { color: ${accent}; }`,
    `img { max-width: 100%; height: auto; }`,
    `pre, code { font-family: ${r(m.text_font_family, "monospace")}; white-space: pre-wrap; }`,
  ];
  m.quote_colors.forEach((color, index) => {
    const selector = Array.from({ length: index + 1 }, () => "blockquote").join(" ");
    rules.push(
      `${selector} { margin: 0 0 0 4px; padding-left: 12px; border-left: 3px solid ${r(color, "currentColor")}; }`,
    );
  });
  if (dark && m.html_dark_mode === "invert") {
    rules.push(
      "html { filter: invert(1) hue-rotate(180deg); } img { filter: invert(1) hue-rotate(180deg); }",
    );
  } else if (dark && m.html_dark_mode === "smart") {
    rules.push(`body * { background-color: transparent !important; color: ${fg} !important; }`);
  }
  return rules.join("\n");
}

/**
 * Full `srcdoc` for a sanitized HTML body. With a `proxy` base, signed
 * `nail-proxy:` URLs load through it; without one they stay unloadable.
 */
export function readerDocument(
  sanitizedHtml: string,
  css: string,
  proxy: string | null = null,
): string {
  const usesProxy = proxy !== null && sanitizedHtml.includes(PROXY_URL_SCHEME);
  const body = usesProxy ? sanitizedHtml.replaceAll(PROXY_URL_SCHEME, proxy) : sanitizedHtml;
  const csp = readerCsp(usesProxy ? originOf(proxy) : null);
  return `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${csp}"><meta name="referrer" content="no-referrer"><style>${css.replaceAll("</", "<\\/")}</style></head><body>${body}</body></html>`;
}
