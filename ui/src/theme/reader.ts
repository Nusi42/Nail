/**
 * The document rendered inside the sandboxed reader iframe.
 *
 * The iframe has `sandbox="allow-same-origin"` — never with scripts (P3,
 * ADR 0164: the reader measures the document's height; `frameHeight.ts`) —
 * and the document carries its own Content-Security-Policy without
 * `script-src`: nothing may be loaded except inline
 * styles, `data:` images/fonts and — for remote content the user allowed —
 * the core's proxy. Inline (`cid:`) images arrive as `data:` URLs: the core
 * resolves them when it builds the body (P1 WP190), because a `srcdoc`
 * document also inherits the page's policy, which allows no `cid:`.
 *
 * The mail's own style sheets come first in the body (the core keeps them,
 * filtered); the reader's rules below give way to them: colours sit at zero
 * specificity (`:where`), and the reading width and the dark-mode handling
 * apply only to mail without its own design (`DESIGNED`).
 *
 * The core's sanitizer removes remote content or, when allowed, rewrites it to signed `nail-proxy:<token>` URLs (ADR 0038),
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
  "default-src 'none'; style-src 'unsafe-inline'; img-src data:; font-src data:; form-action 'none'; base-uri 'none'";

/** CSP of the reader document that may load through the proxy at `origin`. */
export function readerCsp(proxyOrigin: string | null): string {
  if (proxyOrigin === null) return READER_CSP;
  return `default-src 'none'; style-src 'unsafe-inline'; img-src data: ${proxyOrigin}; font-src data: ${proxyOrigin}; form-action 'none'; base-uri 'none'`;
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

/**
 * Inside the reader document: what marks mail that brings its own design —
 * a style sheet (the core places it first in the body), layout tables or
 * its own backgrounds. Such mail keeps its width and colours.
 */
const DESIGNED = ":has(> style, table, [bgcolor], [background], [style*='background'])";

/** The same test on the sanitized HTML, for the frame's surroundings. */
export function isDesignedMail(sanitizedHtml: string): boolean {
  return /<style[\s>]|<table[\s>]|\s(bgcolor|background)=|style="[^"]*background/i.test(
    sanitizedHtml,
  );
}

/** The token mode whose colours paint designed mail's light paper. */
const PAPER_MODE = "print";

/**
 * Reader stylesheet for `mode`, with all token references resolved.
 * `designed` = the mail brings its own design (`isDesignedMail`, decided
 * outside the frame: CSS cannot nest `:has()`, so the document cannot test
 * it on `:root`). In the smart dark mode such mail is drawn on a light
 * "paper" in the theme's print colours, with a light colour scheme for its
 * links, scrollbars and form controls.
 */
export function readerCss(theme: ResolvedTheme, mode: string, designed = false): string {
  const r = (value: string, fallback: string) =>
    safe(resolveTokenRefs(value, theme, mode), fallback);
  const m = theme.message;
  const fg = r("var(--nail-color-fg)", "CanvasText");
  const bg = r("var(--nail-color-bg)", "Canvas");
  const muted = r("var(--nail-color-fg-muted)", "GrayText");
  const subtle = r("var(--nail-color-bg-subtle)", "Canvas");
  const accent = r("var(--nail-color-accent)", "LinkText");
  const dark = mode === "dark" || mode === "high-contrast";
  const paper = designed && mode === "dark" && m.html_dark_mode === "smart";
  const rules = [
    `:root { color-scheme: ${dark && !paper ? "dark" : "light"}; }`,
    `body { margin: 0; padding: 0 0 24px; font-family: ${r(m.font_family, "sans-serif")}; font-size: ${r(m.font_size, "14px")}; line-height: ${r(m.line_height, "1.5")}; overflow-wrap: anywhere; }`,
    // Colours at zero specificity: the mail's own rules always win.
    `:where(body) { color: ${fg}; background-color: ${bg}; }`,
    `:where(a) { color: ${accent}; }`,
    // The reading width only for mail without its own layout.
    `body:not(${DESIGNED}) > * { max-width: ${r(m.max_width, "none")}; }`,
    `img { max-width: 100%; }`,
    // Without a source (blocked, missing) an image keeps its width / height attributes as a placeholder box.
    // An image with a height attribute keeps it after loading too, like other mail programs: spacers and
    // pictures whose own proportions differ from the attributes must not grow (ADR 0144).
    `img[src]:not([height]) { height: auto; }`,
    // Blocked, missing or failed images: their alt text in a quiet frame
    // instead of the browser's broken-image icon (Chromium draws ::after
    // only for images that did not load).
    `img:not([src])[alt]:not([alt=""]) { outline: 1px dashed ${muted}; outline-offset: -1px; color: ${muted}; font-size: 0.85em; }`,
    `img[alt] { position: relative; }`,
    `img[alt]::after { content: attr(alt); position: absolute; inset: 0; display: flex; align-items: center; justify-content: center; overflow: hidden; padding: 2px; box-sizing: border-box; border: 1px dashed ${muted}; font-size: 0.85em; line-height: 1.2; text-align: center; color: ${muted}; background-color: ${subtle}; }`,
    `img[alt=""]::after { border: none; background-color: transparent; }`,
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
  } else if (mode === "high-contrast" && m.html_dark_mode !== "off") {
    // High contrast forces the theme's colours on every mail, designed or not.
    rules.push(
      `html, body { background-color: ${bg} !important; color: ${fg} !important; }`,
      `body * { background-color: transparent !important; color: ${fg} !important; }`,
      `a, a * { color: ${accent} !important; }`,
    );
  } else if (dark && m.html_dark_mode === "smart") {
    if (paper) {
      // Designed mail keeps its own colours on a light "paper", as its author
      // drew it: the theme's print colours (white paper, dark text, a link
      // colour readable on white), after the rules above so they win.
      // A theme without print colours gets a plain white paper.
      const p = (value: string, fallback: string) =>
        !(PAPER_MODE in theme.modes)
          ? fallback
          : safe(resolveTokenRefs(value, theme, PAPER_MODE), fallback);
      rules.push(
        `:where(body) { color: ${p("var(--nail-color-fg)", "#1f1f1f")}; background-color: ${p("var(--nail-color-bg)", "#ffffff")}; }`,
        `:where(a) { color: ${p("var(--nail-color-accent)", "#0b57d0")}; }`,
        // The text keeps off the paper's edge (the mail's own body padding
        // still wins: its style comes later).
        `body { padding: 12px 16px 24px; }`,
      );
    } else {
      // Plain mail takes the theme's colours.
      rules.push(
        `body:not(${DESIGNED}) * { background-color: transparent !important; color: ${fg} !important; }`,
      );
    }
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
