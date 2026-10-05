import { screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { createEnv, renderNode, widget } from "../test/harness";
import { READER_CSP, proxyBase, readerCsp, readerDocument } from "./reader";

describe("remote content in the reader document", () => {
  it("points signed proxy URLs at the page's proxy and allows only that origin", () => {
    const html =
      '<img src="nail-proxy:abc.def" alt=""><p style="background: url(&quot;nail-proxy:ghi.jkl&quot;)">x</p>';
    const doc = readerDocument(html, "body{}", "https://nail.local:4780/api/proxy/");
    expect(doc).toContain('src="https://nail.local:4780/api/proxy/abc.def"');
    expect(doc).toContain("https://nail.local:4780/api/proxy/ghi.jkl");
    expect(doc).not.toContain("nail-proxy:");
    expect(doc).toContain(readerCsp("https://nail.local:4780"));
    expect(readerCsp("https://nail.local:4780")).toContain(
      "img-src data: https://nail.local:4780;",
    );
  });

  it("keeps the closed CSP when nothing is proxied", () => {
    expect(readerDocument("<p>Hi</p>", "", "http://127.0.0.1:1/api/proxy/")).toContain(READER_CSP);
    // Without a proxy the URLs stay unloadable (not http).
    const doc = readerDocument('<img src="nail-proxy:a.b">', "", null);
    expect(doc).toContain(READER_CSP);
  });

  it("uses the page's own proxy route in the browser", () => {
    expect(proxyBase()).toBe(`${window.location.origin}/api/proxy/`);
  });
});

describe("Trusted Types", () => {
  it("sets srcdoc through the nail-reader policy when the browser enforces them", async () => {
    vi.resetModules();
    const created: string[] = [];
    const policy = { createHTML: (s: string) => ({ trusted: s, toString: () => s }) };
    (globalThis as { trustedTypes?: unknown }).trustedTypes = {
      createPolicy: (name: string) => {
        created.push(name);
        return policy;
      },
    };
    try {
      const { setSrcdoc, trustedHtml } = await import("./trustedHtml");
      const frame = document.createElement("iframe");
      setSrcdoc(frame, "<p>x</p>");
      expect(frame.getAttribute("srcdoc")).toBe("<p>x</p>");
      expect(trustedHtml("<b>y</b>")).toMatchObject({ trusted: "<b>y</b>" });
      expect(created).toEqual(["nail-reader"]);
    } finally {
      delete (globalThis as { trustedTypes?: unknown }).trustedTypes;
      vi.resetModules();
    }
  });
});

describe("the remote UI's reader", () => {
  it("frames the message from the sandbox origin with the reader stylesheet", async () => {
    const env = createEnv();
    const message = env.transport.data.messages.find(
      (m) => env.transport.data.bodies.get(m.id)?.sanitized_html,
    );
    if (message === undefined) throw new Error("no HTML message");
    const body = env.transport.data.bodies.get(message.id);
    if (body === undefined) throw new Error("no body");
    body.render_url = "https://nail.local:50001/render/tok.en";
    env.store.getState().selectMessage(message.id);
    const { container } = renderNode(widget("message-view"), env);
    await screen.findByRole("heading");
    const frame = container.querySelector("iframe");
    const src = new URL(frame?.getAttribute("src") ?? "");
    expect(src.origin).toBe("https://nail.local:50001");
    expect(src.pathname).toBe("/render/tok.en");
    expect(src.searchParams.get("css")).toContain("body");
    // P3 (ADR 0164): the app's origin as the height script's target, and
    // scripts (only the route's hash-pinned one) in an opaque origin.
    expect(src.searchParams.get("parent")).toBe(window.location.origin);
    expect(frame).toHaveAttribute("sandbox", "allow-scripts");
    expect(frame?.getAttribute("srcdoc")).toBeNull();
  });
});
