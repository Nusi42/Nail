/** @jsxImportSource @nail/plugin-api */
import { describe, expect, it } from "vitest";

import type { PluginUiNode } from "./generated";
import { Fragment, isUiNode, jsx, jsxDEV, jsxs, toNodes, type UiChild } from "./jsx-runtime";

describe("jsx runtime", () => {
  it("turns intrinsic elements into nodes with their props", () => {
    const node = <date id="due" label="Due" default="tomorrow" required />;
    expect(node).toEqual({
      type: "date",
      id: "due",
      label: "Due",
      default: "tomorrow",
      required: true,
    });
  });

  it("takes the text of <text> and <markdown> from children", () => {
    const subject = "Invoice 42";
    expect(<text style="heading">Remind me about “{subject}”</text>).toEqual({
      type: "text",
      style: "heading",
      value: "Remind me about “Invoice 42”",
    });
    expect(<markdown>**{3}** open</markdown>).toEqual({ type: "markdown", value: "**3** open" });
    expect(<text value="as prop" />).toEqual({ type: "text", value: "as prop" });
    expect(<text />).toEqual({ type: "text", value: "" });
  });

  it("builds stacks, turning strings into text nodes and dropping empties", () => {
    const show = false;
    const tree = (
      <stack direction="horizontal">
        plain
        {show && <text>hidden</text>}
        {null}
        {undefined}
        {""}
        {[<button id="a" label="A" />, <button id="b" label="B" variant="primary" />]}
        <stack>
          <progress value={40} label="Loading" />
        </stack>
      </stack>
    );
    expect(tree).toEqual({
      type: "stack",
      direction: "horizontal",
      children: [
        { type: "text", value: "plain" },
        { type: "button", id: "a", label: "A" },
        { type: "button", id: "b", label: "B", variant: "primary" },
        { type: "stack", children: [{ type: "progress", value: 40, label: "Loading" }] },
      ],
    });
  });

  it("gives an empty stack an empty children list", () => {
    expect(<stack />).toEqual({ type: "stack", children: [] });
  });

  it("flattens fragments and supports function components", () => {
    function Field(props: { name: string; label: string }) {
      return <input id={props.name} label={props.label} kind="email" />;
    }
    function Pair(props: { children?: UiChild }) {
      return (
        <>
          <text>Header</text>
          {props.children}
        </>
      );
    }
    const content = (
      <Pair>
        <Field name="to" label="To" />
        <checkbox id="cc" label="Copy me" default={false} />
      </Pair>
    );
    expect(toNodes(content)).toEqual([
      { type: "text", value: "Header" },
      { type: "input", id: "to", label: "To", kind: "email" },
      { type: "checkbox", id: "cc", label: "Copy me", default: false },
    ]);
  });

  it("omits undefined props and the key", () => {
    const placeholder: string | undefined = undefined;
    const node = jsx("input", { id: "n", label: "Name", placeholder }, "k1");
    expect(node).toEqual({ type: "input", id: "n", label: "Name" });
    expect(Object.keys(node as object)).not.toContain("placeholder");
  });

  it("renders every node type of the contract", () => {
    const nodes = toNodes(
      <>
        <text>t</text>
        <markdown>m</markdown>
        <input id="i" label="I" />
        <textarea id="ta" label="TA" rows={3} />
        <select id="s" label="S" options={[{ value: "a", label: "A" }]} default={["a"]} />
        <checkbox id="c" label="C" />
        <date id="d" label="D" time />
        <contact-picker id="cp" label="CP" multiple />
        <folder-picker id="fp" label="FP" accountId="work" />
        <message-list id="ml" messageIds={["m1"]} selectable />
        <table columns={[{ label: "A" }]} rows={[["1"]]} />
        <button id="b" label="B" />
        <progress />
        <image src="data:image/png;base64,AA==" alt="" />
        <stack />
      </>,
    );
    expect(nodes.map((n) => n.type)).toEqual([
      "text",
      "markdown",
      "input",
      "textarea",
      "select",
      "checkbox",
      "date",
      "contact-picker",
      "folder-picker",
      "message-list",
      "table",
      "button",
      "progress",
      "image",
      "stack",
    ]);
    // The trees are plain JSON.
    expect(JSON.parse(JSON.stringify(nodes))).toEqual(nodes);
  });

  it("rejects unknown elements, children of leaves and mixed text", () => {
    expect(() => jsx("div", {})).toThrow(/Unknown Nail UI element <div>/);
    expect(() => jsx("button", { id: "b", label: "B", children: "x" })).toThrow(
      /<button> takes no children/,
    );
    expect(() =>
      jsx("text", { children: [{ type: "text", value: "nested" }] as unknown as string }),
    ).toThrow(/<text> takes text only/);
    expect(() => jsx("text", { value: "a", children: "b" })).toThrow(/either a value or children/);
    expect(() => toNodes({ nope: true } as unknown as UiChild)).toThrow(/Not a Nail UI node/);
  });

  it("does not let a prop override the node type", () => {
    const node = jsx("checkbox", { id: "x", label: "X", type: "button" });
    expect((node as PluginUiNode).type).toBe("checkbox");
  });

  it("exports jsxs, jsxDEV and Fragment for the automatic runtime", () => {
    expect(jsxs("stack", { children: ["a", "b"] })).toEqual({
      type: "stack",
      children: [
        { type: "text", value: "a" },
        { type: "text", value: "b" },
      ],
    });
    expect(jsxDEV("text", { children: "dev" }, undefined, false, {}, undefined)).toEqual({
      type: "text",
      value: "dev",
    });
    expect(jsx(Fragment, { children: ["x", 1, true] })).toEqual([
      { type: "text", value: "x" },
      { type: "text", value: "1" },
    ]);
    expect(jsx(() => null, {})).toEqual([]);
  });

  it("recognizes nodes", () => {
    expect(isUiNode({ type: "text", value: "" })).toBe(true);
    expect(isUiNode({ type: "div" })).toBe(false);
    expect(isUiNode([])).toBe(false);
    expect(isUiNode(null)).toBe(false);
  });
});
