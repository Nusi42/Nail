import { describe, expect, it } from "vitest";

import { fromBase64 } from "./bytes";
import {
  addContent,
  arrivalProblems,
  badge,
  banner,
  beforeSendProblems,
  createdId,
  isDone,
  isHeaderValue,
  isPluginHeaderName,
  isPreview,
  markdownBlockProblems,
  MAX_ADDED_CONTENT,
  mergeBeforeSend,
  openProblems,
  previewImage,
  previewProblems,
  previewTree,
  veto,
} from "./extensions";
import type { PluginArrivalResult } from "./generated";

describe("before-send answers", () => {
  it("builds vetoes and content and merges answers", () => {
    expect(veto("No attachment")).toEqual({ veto: { reason: "No attachment" } });
    expect(addContent("-- sent with care", { position: "bottom", html: "<p>…</p>" })).toEqual({
      addContent: [{ text: "-- sent with care", position: "bottom", html: "<p>…</p>" }],
    });
    const merged = mergeBeforeSend(
      null,
      { headers: [{ name: "X-Ticket", value: "1" }] },
      veto("first"),
      veto("second"),
      addContent("x"),
    );
    expect(merged).toEqual({
      veto: { reason: "first" },
      headers: [{ name: "X-Ticket", value: "1" }],
      addContent: [{ text: "x" }],
    });
  });

  it("checks answers like Nail does", () => {
    expect(beforeSendProblems(undefined)).toEqual([]);
    expect(beforeSendProblems(veto("ok"))).toEqual([]);
    expect(beforeSendProblems(veto(" "))).toEqual(["veto reason empty or too long"]);
    expect(beforeSendProblems(veto("x".repeat(501)))).toEqual(["veto reason empty or too long"]);
    expect(beforeSendProblems(veto("😀".repeat(500)))).toEqual([]); // counted in characters
    const headers = Array.from({ length: 11 }, (_, i) => ({ name: `X-H${i}`, value: "v" }));
    expect(beforeSendProblems({ headers })).toEqual(["too many headers"]);
    expect(beforeSendProblems({ headers: [{ name: "Subject", value: "x" }] })).toEqual([
      "invalid header",
    ]);
    expect(beforeSendProblems(addContent("é".repeat(MAX_ADDED_CONTENT / 2 + 1)))).toEqual([
      "added content too large",
    ]);
  });

  it("allows X- headers only, never Nail's own", () => {
    expect(isPluginHeaderName("X-Crm-Ticket")).toBe(true);
    expect(isPluginHeaderName("x-lower")).toBe(true);
    for (const name of [
      "X-",
      "Subject",
      "X-Nail-Loop",
      "x-nail-anything",
      "X-Bad:Name",
      "X-Sp ace",
      `X-${"a".repeat(80)}`,
    ]) {
      expect(isPluginHeaderName(name), name).toBe(false);
    }
    expect(isHeaderValue("one line")).toBe(true);
    expect(isHeaderValue("two\r\nlines")).toBe(false);
    expect(isHeaderValue("nul\0")).toBe(false);
    expect(isHeaderValue("x".repeat(999))).toBe(false);
  });
});

describe("arrival and open answers", () => {
  it("checks arrival actions", () => {
    expect(arrivalProblems(null)).toEqual([]);
    expect(
      arrivalProblems({ actions: [{ type: "addTag", tag: "crm" }, { type: "archive" }] }),
    ).toEqual([]);
    const many: PluginArrivalResult = {
      actions: Array.from({ length: 21 }, () => ({ type: "setRead" as const, read: true })),
    };
    expect(arrivalProblems(many)).toEqual(["too many actions"]);
    expect(
      arrivalProblems({ actions: [{ type: "delete" }] } as unknown as PluginArrivalResult),
    ).toEqual(["unknown action"]);
  });

  it("builds and checks banners and badges", () => {
    const b = banner("unverified", "The sender is new", {
      level: "warning",
      buttons: [{ id: "trust", label: "Trust" }],
    });
    expect(b).toEqual({
      id: "unverified",
      text: "The sender is new",
      level: "warning",
      buttons: [{ id: "trust", label: "Trust" }],
    });
    expect(badge("CRM", { icon: "tag", tooltip: "In the CRM" })).toEqual({
      text: "CRM",
      icon: "tag",
      tooltip: "In the CRM",
    });
    expect(openProblems({ banners: [b], badges: [badge("CRM")] })).toEqual([]);
    expect(openProblems({ banners: [banner("Bad Id", "x")] })).toEqual(["invalid banner"]);
    expect(
      openProblems({
        banners: [
          banner("a", "x", {
            buttons: ["a", "b", "c", "d"].map((id) => ({ id, label: id })),
          }),
        ],
      }),
    ).toEqual(["invalid banner"]);
    expect(openProblems({ badges: [badge("x".repeat(33))] })).toEqual(["invalid badge"]);
    expect(openProblems({ badges: ["a", "b", "c", "d"].map((t) => badge(t)) })).toEqual([
      "too many banners or badges",
    ]);
  });
});

describe("previews", () => {
  it("makes trees from JSX and images from bytes", () => {
    const tree = previewTree(
      <stack>
        <text>Rows: 2</text>
      </stack>,
    );
    expect(tree).toEqual({
      kind: "tree",
      tree: [{ type: "stack", children: [{ type: "text", value: "Rows: 2" }] }],
    });
    const png = Uint8Array.from([0x89, 0x50, 0x4e, 0x47]);
    const image = previewImage("image/png", png, "A chart");
    expect(image).toMatchObject({ kind: "image", mimeType: "image/png", alt: "A chart" });
    expect(image.kind === "image" && fromBase64(image.data)).toEqual(png);
    expect(previewImage("image/gif", "R0lGOD")).toEqual({
      kind: "image",
      mimeType: "image/gif",
      data: "R0lGOD",
    });
    expect(isPreview(tree)).toBe(true);
    expect(isPreview(image)).toBe(true);
    expect(isPreview({ type: "text", value: "x" })).toBe(false);
    expect(isPreview([tree])).toBe(false);
  });

  it("refuses SVG, oversized images and bad trees", () => {
    expect(previewProblems(previewTree("fine"))).toEqual([]);
    expect(previewProblems({ kind: "image", mimeType: "image/svg+xml", data: "" })).toEqual([
      "image type not allowed",
    ]);
    expect(
      previewProblems({ kind: "image", mimeType: "image/png", data: "A".repeat(7 * 1024 * 1024) }),
    ).toEqual(["image too large"]);
    expect(previewProblems(previewTree(<button id="" label="x" />))[0]).toMatch(/needs an id/);
    expect(previewProblems(null)).toEqual(["not a preview"]);
    expect(previewProblems({ kind: "html", html: "<b>" } as never)).toEqual(["not a preview"]);
  });

  it("checks Markdown block answers", () => {
    expect(markdownBlockProblems({ html: "<b>x</b>", text: "x" })).toEqual([]);
    expect(markdownBlockProblems({ html: "x".repeat(256 * 1024 + 1), text: "" })).toEqual([
      "html too large",
    ]);
    expect(markdownBlockProblems({ text: "x" } as never)).toEqual(["not a block result"]);
  });
});

describe("write results", () => {
  it("tells done writes apart", () => {
    const done = { status: "done" as const, opIds: ["op-1"], created: ["task-1"] };
    expect(isDone(done)).toBe(true);
    expect(createdId(done)).toBe("task-1");
    const denied = { status: "denied" as const, confirmationId: "c1" };
    expect(isDone(denied)).toBe(false);
    expect(createdId(denied)).toBeUndefined();
  });
});
