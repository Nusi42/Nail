/** Mail frames sized to their content (P3 WP-card-height, ADR 0164). */
import { act } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { createEnv, Providers, renderWithProviders } from "../test/harness";
import { must } from "../test/must";
import { FRAME_HEIGHT_MESSAGE, MAX_FRAME_HEIGHT } from "../theme/frameHeight";
import { LocalMailFrame, RemoteMailFrame, renderSrc } from "./MailFrame";

const SANDBOX = "https://nail.local:50001";
const RENDER_URL = `${SANDBOX}/render/tok.en`;
const TITLE = "Message content";

/** The height the geometry rule gives `frame`, if any. */
function geometryHeight(frame: HTMLIFrameElement): string | null {
  const geom = frame.getAttribute("data-geom");
  const sheet = document.querySelector<HTMLStyleElement>("style[data-nail-theme]")?.sheet;
  if (geom === null || !sheet) return null;
  for (const rule of Array.from(sheet.cssRules)) {
    if (rule instanceof CSSStyleRule && rule.selectorText.includes(geom)) {
      const height = rule.style.getPropertyValue("height");
      return height === "" ? null : height;
    }
  }
  return null;
}

/** Gives `doc` the sizes a laid-out document would report. */
function layOut(
  doc: Document,
  sizes: { client: number; scroll: number; offset: number; inner: number },
): void {
  const root = doc.documentElement;
  Object.defineProperty(root, "clientHeight", { configurable: true, value: sizes.client });
  Object.defineProperty(root, "scrollHeight", { configurable: true, value: sizes.scroll });
  Object.defineProperty(root, "offsetHeight", { configurable: true, value: sizes.offset });
  Object.defineProperty(must(doc.defaultView), "innerHeight", {
    configurable: true,
    value: sizes.inner,
  });
}

/** Turns the frame's document into its `srcdoc` document (jsdom does not load srcdoc). */
function loadSrcdoc(frame: HTMLIFrameElement): Document {
  const doc = must(frame.contentDocument);
  Object.defineProperty(doc, "URL", { configurable: true, value: "about:srcdoc" });
  return doc;
}

describe("the local mail frame", () => {
  it("is sandboxed without scripts, keeps its srcdoc and sizes itself to the document", () => {
    const env = createEnv();
    const { container } = renderWithProviders(
      <LocalMailFrame srcDoc="<p>Hi</p>" title={TITLE} designed={false} />,
      env,
    );
    const frame = must(container.querySelector("iframe"));
    expect(frame).toHaveAttribute("sandbox", "allow-same-origin");
    expect(frame.getAttribute("sandbox")).not.toContain("allow-scripts");
    expect(frame).toHaveAttribute("srcdoc", "<p>Hi</p>");
    expect(frame).toHaveAttribute("data-part", "message-view.body");
    // The initial about:blank is not measured: a placeholder until then.
    expect(frame.getAttribute("data-state")).toBeNull();
    expect(geometryHeight(frame)).toBeNull();

    const doc = loadSrcdoc(frame);
    layOut(doc, { client: 192, scroll: 192, offset: 74, inner: 192 });
    act(() => {
      frame.dispatchEvent(new Event("load"));
    });
    // A short mail: the root's own height, smaller than the placeholder.
    expect(geometryHeight(frame)).toBe("74px");
    expect(frame).toHaveAttribute("data-state", "measured");

    // An image loads and the content overflows: the frame grows to it.
    layOut(doc, { client: 74, scroll: 540, offset: 540, inner: 74 });
    act(() => {
      doc.body.dispatchEvent(new Event("load"));
    });
    expect(geometryHeight(frame)).toBe("540px");

    // A horizontal scrollbar (wide mail) adds its thickness.
    layOut(doc, { client: 525, scroll: 540, offset: 540, inner: 540 });
    act(() => {
      doc.body.dispatchEvent(new Event("error"));
    });
    expect(geometryHeight(frame)).toBe("555px");
  });

  it("marks designed mail and measures again after a new document", () => {
    const env = createEnv();
    const { container, rerender } = renderWithProviders(
      <LocalMailFrame srcDoc="<table></table>" title={TITLE} designed />,
      env,
    );
    const frame = must(container.querySelector("iframe"));
    expect(frame).toHaveAttribute("data-state", "designed");
    layOut(loadSrcdoc(frame), { client: 192, scroll: 192, offset: 300, inner: 192 });
    act(() => {
      frame.dispatchEvent(new Event("load"));
    });
    expect(frame).toHaveAttribute("data-state", "designed measured");
    expect(geometryHeight(frame)).toBe("300px");

    // A theme switch: a new srcdoc, a new document; the old height stays until it is measured.
    rerender(
      <Providers env={env}>
        <LocalMailFrame srcDoc="<table> </table>" title={TITLE} designed />
      </Providers>,
    );
    expect(frame).toHaveAttribute("srcdoc", "<table> </table>");
    expect(geometryHeight(frame)).toBe("300px");
    layOut(loadSrcdoc(frame), { client: 300, scroll: 300, offset: 120, inner: 300 });
    act(() => {
      frame.dispatchEvent(new Event("load"));
    });
    expect(geometryHeight(frame)).toBe("120px");
  });
});

describe("the remote mail frame", () => {
  it("names the app origin as the parent and runs only the route's script, in an opaque origin", () => {
    const src = new URL(renderSrc(RENDER_URL, "body{}", "https://nail.local:50000"));
    expect(src.origin).toBe(SANDBOX);
    expect(src.searchParams.get("css")).toBe("body{}");
    expect(src.searchParams.get("parent")).toBe("https://nail.local:50000");

    const env = createEnv();
    const { container } = renderWithProviders(
      <RemoteMailFrame renderUrl={RENDER_URL} css="body{}" title={TITLE} designed />,
      env,
    );
    const frame = must(container.querySelector("iframe"));
    expect(frame).toHaveAttribute("sandbox", "allow-scripts");
    expect(frame.getAttribute("sandbox")).not.toContain("allow-same-origin");
    expect(new URL(frame.src).searchParams.get("parent")).toBe(window.location.origin);
  });

  it("takes the height only from its own frame's window", () => {
    const env = createEnv();
    const { container } = renderWithProviders(
      <RemoteMailFrame renderUrl={RENDER_URL} css="body{}" title={TITLE} designed={false} />,
      env,
    );
    const frame = must(container.querySelector("iframe"));
    const post = (data: unknown, source: unknown, origin: string) => {
      act(() => {
        window.dispatchEvent(
          new MessageEvent("message", { data, source: source as Window, origin }),
        );
      });
    };
    // Another window, or a foreign origin: ignored.
    post({ type: FRAME_HEIGHT_MESSAGE, height: 900 }, window, "null");
    post({ type: FRAME_HEIGHT_MESSAGE, height: 900 }, frame.contentWindow, "https://evil.example");
    expect(geometryHeight(frame)).toBeNull();
    // Its own frame, from the opaque origin the sandbox gives it.
    post({ type: FRAME_HEIGHT_MESSAGE, height: 412.2 }, frame.contentWindow, "null");
    expect(geometryHeight(frame)).toBe("413px");
    expect(frame).toHaveAttribute("data-state", "measured");
    // Clamped: a taller document scrolls inside.
    post({ type: FRAME_HEIGHT_MESSAGE, height: 1e9 }, frame.contentWindow, SANDBOX);
    expect(geometryHeight(frame)).toBe(`${String(MAX_FRAME_HEIGHT)}px`);
  });
});
