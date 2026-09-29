/**
 * The document rendered inside the sandboxed reader iframe.
 *
 * The iframe has `sandbox=""` (no scripts, opaque origin) and the document
 * carries its own Content-Security-Policy: nothing may be loaded except inline
 * styles and `data:` images/fonts. Remote content is already stripped by the
 * core's sanitizer; the CSP is the second line of defence.
 */
import type { ResolvedTheme } from "../api/generated";
import { isSafeValue, resolveTokenRefs } from "./compile";

/** CSP of the reader document. */
export const READER_CSP =
  "default-src 'none'; style-src 'unsafe-inline'; img-src data: cid:; font-src data:; form-action 'none'; base-uri 'none'";

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

/** Full `srcdoc` for a sanitized HTML body. */
export function readerDocument(sanitizedHtml: string, css: string): string {
  return `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${READER_CSP}"><meta name="referrer" content="no-referrer"><style>${css.replaceAll("</", "<\\/")}</style></head><body>${sanitizedHtml}</body></html>`;
}
