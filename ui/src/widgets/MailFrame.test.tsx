/** Mail frames sized to their content (P3 WP-card-height, ADR 0164; ADR 0212). */
import { act } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { createEnv, Providers, renderWithProviders } from "../test/harness";
import { must } from "../test/must";
import { FRAME_LINK_MESSAGE, FRAME_MENU_MESSAGE } from "../api/generated";
import {
  FRAME_HEIGHT_MESSAGE,
  FRAME_MENU_EVENT,
  MAX_FRAME_HEIGHT,
  OPAQUE_ORIGIN,
} from "../theme/frameHeight";
import { LocalMailFrame, RemoteMailFrame, renderSrc } from "./MailFrame";
import type { MailLinkReport } from "./mailLinks";

const SANDBOX = "https://nail.local:50001";
const RENDER_URL = `${SANDBOX}/render/tok.en`;
const TITLE = "Message content";

/** A frame whose links nobody follows. */
const ignore = () => undefined;

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

/** Posts `data` to the reader's window as `source` from `origin`. */
function postFrom(data: unknown, source: unknown, origin: string) {
  act(() => {
    window.dispatchEvent(new MessageEvent("message", { data, source: source as Window, origin }));
  });
}

/** A remote frame and a way to post messages as `source` from `origin`. */
function remoteFrame(onLink: (report: MailLinkReport) => void) {
  const { container } = renderWithProviders(
    <RemoteMailFrame
      renderUrl={RENDER_URL}
      css="body{}"
      title={TITLE}
      designed={false}
      onLink={onLink}
    />,
    createEnv(),
  );
  const frame = must(container.querySelector("iframe"));
  return { frame, post: postFrom };
}

/** A local frame showing a short mail, whose links nobody follows. */
function localFrame(): HTMLIFrameElement {
  const { container } = renderWithProviders(
    <LocalMailFrame srcDoc="<p>Hi</p>" title={TITLE} designed={false} onLink={ignore} />,
    createEnv(),
  );
  return must(container.querySelector("iframe"));
}

describe("the local mail frame", () => {
  it("is sandboxed in an opaque origin, keeps its srcdoc and takes heights from its document", () => {
    const frame = localFrame();
    // ADR 0212: scripts (only the frame script), never the app's origin.
    expect(frame).toHaveAttribute("sandbox", "allow-scripts");
    expect(frame).toHaveAttribute("srcdoc", "<p>Hi</p>");
    expect(frame).toHaveAttribute("data-part", "message-view.body");
    // A placeholder until the document reports.
    expect(frame.getAttribute("data-state")).toBeNull();
    expect(geometryHeight(frame)).toBeNull();

    // Another window, or the sandbox origin of a remote frame: ignored.
    postFrom({ type: FRAME_HEIGHT_MESSAGE, height: 900 }, window, OPAQUE_ORIGIN);
    postFrom({ type: FRAME_HEIGHT_MESSAGE, height: 900 }, frame.contentWindow, SANDBOX);
    expect(geometryHeight(frame)).toBeNull();
    postFrom({ type: FRAME_HEIGHT_MESSAGE, height: 74 }, frame.contentWindow, OPAQUE_ORIGIN);
    expect(geometryHeight(frame)).toBe("74px");
    expect(frame).toHaveAttribute("data-state", "measured");
  });

  it("marks designed mail and starts over with a new document", () => {
    const env = createEnv();
    const { container, rerender } = renderWithProviders(
      <LocalMailFrame srcDoc="<table></table>" title={TITLE} designed onLink={ignore} />,
      env,
    );
    const frame = must(container.querySelector("iframe"));
    expect(frame).toHaveAttribute("data-state", "designed");
    postFrom({ type: FRAME_HEIGHT_MESSAGE, height: 300 }, frame.contentWindow, OPAQUE_ORIGIN);
    expect(frame).toHaveAttribute("data-state", "designed measured");
    expect(geometryHeight(frame)).toBe("300px");

    // A theme switch: a new srcdoc, a new document; the old height stays until it reports.
    rerender(
      <Providers env={env}>
        <LocalMailFrame srcDoc="<table> </table>" title={TITLE} designed onLink={ignore} />
      </Providers>,
    );
    expect(frame).toHaveAttribute("srcdoc", "<table> </table>");
    expect(geometryHeight(frame)).toBe("300px");
    postFrom({ type: FRAME_HEIGHT_MESSAGE, height: 120 }, frame.contentWindow, OPAQUE_ORIGIN);
    expect(geometryHeight(frame)).toBe("120px");
  });

  it("turns its document's menu request into an event on the iframe", () => {
    const frame = localFrame();
    const seen = vi.fn();
    document.addEventListener(FRAME_MENU_EVENT, seen);
    const menu = { type: FRAME_MENU_MESSAGE, x: 5, y: 7, button: 2, text: "Hi" };
    postFrom(menu, window, OPAQUE_ORIGIN);
    expect(seen).not.toHaveBeenCalled();
    postFrom(menu, frame.contentWindow, OPAQUE_ORIGIN);
    document.removeEventListener(FRAME_MENU_EVENT, seen);
    expect(seen).toHaveBeenCalledTimes(1);
    const event = seen.mock.calls[0]?.[0] as CustomEvent;
    expect(event.target).toBe(frame);
    expect(event.detail).toEqual({ x: 5, y: 7, button: 2, text: "Hi" });
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
      <RemoteMailFrame
        renderUrl={RENDER_URL}
        css="body{}"
        title={TITLE}
        designed
        onLink={ignore}
      />,
      env,
    );
    const frame = must(container.querySelector("iframe"));
    expect(frame).toHaveAttribute("sandbox", "allow-scripts");
    expect(frame.getAttribute("sandbox")).not.toContain("allow-same-origin");
    expect(new URL(frame.src).searchParams.get("parent")).toBe(window.location.origin);
  });

  it("takes the height only from its own frame's window", () => {
    const { frame, post } = remoteFrame(ignore);
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

describe("links in mail frames (ADR 0206)", () => {
  it("takes link reports only from the remote frame's own window", () => {
    const onLink = vi.fn();
    const { frame, post } = remoteFrame(onLink);
    const hover = { type: FRAME_LINK_MESSAGE, open: false, href: "https://example.com/", index: 3 };
    post(hover, window, "null");
    post(hover, frame.contentWindow, "https://evil.example");
    post({ ...hover, index: "3" }, frame.contentWindow, "null");
    post({ ...hover, href: null, open: true }, frame.contentWindow, "null");
    expect(onLink).not.toHaveBeenCalled();
    post(hover, frame.contentWindow, "null");
    expect(onLink).toHaveBeenLastCalledWith({
      link: { href: "https://example.com/", index: 3 },
      open: false,
    });
    post({ ...hover, open: true }, frame.contentWindow, SANDBOX);
    expect(onLink).toHaveBeenLastCalledWith({
      link: { href: "https://example.com/", index: 3 },
      open: true,
    });
    post(
      { type: FRAME_LINK_MESSAGE, open: false, href: null, index: -1 },
      frame.contentWindow,
      "null",
    );
    expect(onLink).toHaveBeenLastCalledWith({ link: null, open: false });
  });
});
