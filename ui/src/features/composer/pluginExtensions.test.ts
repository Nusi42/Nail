import { describe, expect, it } from "vitest";
import { applyEditToBody, pluginFences, textAsHtml } from "./pluginExtensions";

const body = { text: "Hello world", html: "<p>Hi</p>", markdown: "# Title\n" };

describe("edits from plug-ins and agents", () => {
  it("apply to the plain text at the selection", () => {
    const next = applyEditToBody(
      "plain",
      body,
      { start: 6, end: 11 },
      { op: "replace-selection", text: "there" },
    );
    expect(next.text).toBe("Hello there");
    expect(next.html).toBe(body.html);
  });

  it("apply to Markdown and leave the other bodies", () => {
    const next = applyEditToBody(
      "markdown",
      body,
      { start: 8, end: 8 },
      { op: "insert", text: "Text" },
    );
    expect(next.markdown).toBe("# Title\nText");
    expect(next.text).toBe(body.text);
  });

  it("append or replace HTML, escaping plain text", () => {
    expect(
      applyEditToBody("html", body, { start: 0, end: 0 }, { op: "insert", text: "a < b" }).html,
    ).toBe("<p>Hi</p><p>a &lt; b</p>");
    expect(
      applyEditToBody(
        "html",
        body,
        { start: 0, end: 0 },
        { op: "replace-all", text: "<b>x</b>", format: "html" },
      ).html,
    ).toBe("<b>x</b>");
    expect(textAsHtml("one\ntwo\n\nthree")).toBe("<p>one<br>two</p><p>three</p>");
  });
});

describe("plug-in Markdown blocks", () => {
  it("are found by their <plugin>.<block> info string", () => {
    const md = [
      "Intro",
      "```chart.bar",
      "a,1",
      "```",
      "```js",
      "x()",
      "```",
      "~~~~kanban.board",
      "~~~",
      "inner",
      "~~~~",
      "```open.block",
      "never closed",
    ].join("\n");
    expect(pluginFences(md)).toEqual([
      { info: "chart.bar", body: "a,1" },
      { info: "kanban.board", body: "~~~\ninner" },
    ]);
  });
});
