/**
 * A tiny JSX runtime producing Nail's declarative component trees
 * (`PluginUiNode`), used through `"jsx": "react-jsx"` +
 * `"jsxImportSource": "@nail/plugin-api"` (TypeScript) or
 * `jsx: "automatic", jsxImportSource: "@nail/plugin-api"` (esbuild).
 *
 * Intrinsic elements are the node types (`<text>`, `<date>`,
 * `<contact-picker>`, `<stack>` …) and take the node's fields as props.
 * `<text>` and `<markdown>` take their `value` from their children,
 * `<stack>` its `children`; strings inside a `<stack>` become `<text>`
 * nodes. Function components and fragments work as in React, but there is
 * no state and no rendering loop: the result is plain data.
 */
import type { PluginUiNode } from "./generated";

/** Every `type` of a {@link PluginUiNode}. */
export type PluginUiNodeType = PluginUiNode["type"];

/** The node with a given `type`. */
export type PluginUiNodeOf<K extends PluginUiNodeType> = Extract<PluginUiNode, { type: K }>;

/** What may appear as JSX children. */
export type UiChild =
  PluginUiNode | string | number | boolean | null | undefined | readonly UiChild[];

/** What a JSX expression evaluates to: one node or several (fragments, components). */
export type UiContent = PluginUiNode | readonly PluginUiNode[];

/** Children of `<text>` and `<markdown>`: text only. */
export type UiTextChild = string | number | boolean | null | undefined | readonly UiTextChild[];

/** Every element accepts a `key` (ignored; there is no reconciliation). */
interface KeyProp {
  key?: string | number;
}

/** Props of an element whose node has no children. */
type LeafProps<K extends PluginUiNodeType> = Omit<PluginUiNodeOf<K>, "type"> &
  KeyProp & { children?: never };

/** Props of `<text>` / `<markdown>`: `value` or text children. */
type TextProps<K extends "text" | "markdown"> = Omit<PluginUiNodeOf<K>, "type" | "value"> &
  KeyProp & { value?: string; children?: UiTextChild };

/** The props of each intrinsic element. */
export interface UiIntrinsicElements {
  text: TextProps<"text">;
  markdown: TextProps<"markdown">;
  input: LeafProps<"input">;
  textarea: LeafProps<"textarea">;
  select: LeafProps<"select">;
  checkbox: LeafProps<"checkbox">;
  date: LeafProps<"date">;
  "contact-picker": LeafProps<"contact-picker">;
  "folder-picker": LeafProps<"folder-picker">;
  "message-list": LeafProps<"message-list">;
  table: LeafProps<"table">;
  button: LeafProps<"button">;
  progress: LeafProps<"progress">;
  image: LeafProps<"image">;
  stack: Omit<PluginUiNodeOf<"stack">, "type" | "children"> & KeyProp & { children?: UiChild };
}

/** A function component: props in, content out. */
export type UiComponent<P = Record<string, unknown>> = (props: P) => UiContent | null;

/**
 * Every node type of the contract. A `Record` over the generated union, so
 * a component added to `PluginUiNode` in Rust fails to compile here until
 * the runtime knows it (and `UiIntrinsicElements` below gives it props).
 */
const NODE_TYPE_TABLE: Record<PluginUiNodeType, "leaf" | "text" | "container"> = {
  text: "text",
  markdown: "text",
  input: "leaf",
  textarea: "leaf",
  select: "leaf",
  checkbox: "leaf",
  date: "leaf",
  "contact-picker": "leaf",
  "folder-picker": "leaf",
  "message-list": "leaf",
  table: "leaf",
  button: "leaf",
  progress: "leaf",
  image: "leaf",
  stack: "container",
};

/** Every node type, in the contract's order. */
export const NODE_TYPES: readonly PluginUiNodeType[] = Object.keys(
  NODE_TYPE_TABLE,
) as PluginUiNodeType[];

const NODE_TYPE_SET: ReadonlySet<string> = new Set<string>(NODE_TYPES);

/** Node types without JSX props (`never` when every node type has them). */
type MissingElements = Exclude<PluginUiNodeType, keyof UiIntrinsicElements>;
/**
 * Compile-time check for `JSX.IntrinsicElements`: when a node type lacks JSX
 * props, every element fails to type-check and the error names the missing
 * types.
 */
type CheckedIntrinsicElements = [MissingElements] extends [never]
  ? UiIntrinsicElements
  : { missingJsxProps: MissingElements };

/** Whether `value` looks like a {@link PluginUiNode} (an object with a known `type`). */
export function isUiNode(value: unknown): value is PluginUiNode {
  return (
    typeof value === "object" &&
    value !== null &&
    !Array.isArray(value) &&
    typeof (value as { type?: unknown }).type === "string" &&
    NODE_TYPE_SET.has((value as { type: string }).type)
  );
}

/**
 * Flattens JSX children or content into a list of nodes: arrays are
 * flattened, `null` / `undefined` / booleans / empty strings dropped,
 * other strings and numbers become `text` nodes.
 */
export function toNodes(content: UiChild): PluginUiNode[] {
  const out: PluginUiNode[] = [];
  const visit = (child: UiChild): void => {
    if (child === null || child === undefined || typeof child === "boolean") return;
    if (Array.isArray(child)) {
      for (const c of child as readonly UiChild[]) visit(c);
      return;
    }
    if (typeof child === "string" || typeof child === "number") {
      // Like React, an empty string renders nothing (`{name && <text>…</text>}`).
      if (child !== "") out.push({ type: "text", value: String(child) });
      return;
    }
    if (isUiNode(child)) {
      out.push(child);
      return;
    }
    throw new TypeError(`Not a Nail UI node: ${describe(child)}`);
  };
  visit(content);
  return out;
}

function describe(value: unknown): string {
  try {
    // `undefined` for functions, symbols and `undefined` itself.
    const json = JSON.stringify(value) as string | undefined;
    return json ?? String(value);
  } catch {
    return String(value);
  }
}

/** Joins text children (`<text>Hello {name}</text>`). */
function childText(children: unknown, element: string): string {
  let text = "";
  const visit = (child: unknown): void => {
    if (child === null || child === undefined || typeof child === "boolean") return;
    if (Array.isArray(child)) {
      for (const c of child) visit(c);
      return;
    }
    if (typeof child === "string" || typeof child === "number") {
      text += String(child);
      return;
    }
    throw new TypeError(`<${element}> takes text only, got ${describe(child)}`);
  };
  visit(children);
  return text;
}

/** Copies props without `children`, `key` and `undefined` values. */
function fields(props: Record<string, unknown>): Record<string, unknown> {
  const out: Record<string, unknown> = {};
  for (const [name, value] of Object.entries(props)) {
    if (name === "children" || name === "key" || value === undefined) continue;
    out[name] = value;
  }
  return out;
}

function hasChildren(children: unknown): boolean {
  return toFlat(children).some((c) => c !== null && c !== undefined && typeof c !== "boolean");
}

function toFlat(children: unknown): unknown[] {
  return Array.isArray(children) ? children.flat(Infinity) : [children];
}

function intrinsic(type: string, props: Record<string, unknown>): PluginUiNode {
  if (!NODE_TYPE_SET.has(type)) {
    throw new TypeError(`Unknown Nail UI element <${type}>`);
  }
  const node: Record<string, unknown> = { type, ...fields(props) };
  const children = props.children;
  const shape = NODE_TYPE_TABLE[type as PluginUiNodeType];
  if (shape === "text") {
    if (hasChildren(children)) {
      if (props.value !== undefined) {
        throw new TypeError(`<${type}> takes either a value or children, not both`);
      }
      node.value = childText(children, type);
    } else if (node.value === undefined) {
      node.value = "";
    }
  } else if (shape === "container") {
    node.children = toNodes(children as UiChild);
  } else if (hasChildren(children)) {
    throw new TypeError(`<${type}> takes no children`);
  }
  // `type` last-wins protection: a prop named `type` must not change the node.
  node.type = type;
  return node as PluginUiNode;
}

/** The fragment component (`<>…</>`): its children as a list of nodes. */
export function Fragment(props: { children?: UiChild }): PluginUiNode[] {
  return toNodes(props.children);
}

type ElementType = string | UiComponent<never>;

/**
 * The JSX factory (automatic runtime). The third argument (`key`) is
 * accepted and ignored.
 */
export const jsx: (
  type: ElementType,
  props: Record<string, unknown> | null,
  key?: unknown,
) => UiContent = (type, props) => {
  const p = props ?? {};
  if (typeof type === "function") {
    return (type as UiComponent)(p) ?? [];
  }
  if (typeof type !== "string") {
    throw new TypeError(`Not a Nail UI element: ${describe(type)}`);
  }
  return intrinsic(type, p);
};

/** The JSX factory for static children lists (same as {@link jsx}). */
export const jsxs = jsx;

/** The development JSX factory (same as {@link jsx}; key and source info ignored). */
export const jsxDEV: (
  type: ElementType,
  props: Record<string, unknown> | null,
  key?: unknown,
  isStatic?: boolean,
  source?: unknown,
  self?: unknown,
) => UiContent = (type, props) => jsx(type, props);

/** TypeScript's view of the JSX in plug-ins. */
// eslint-disable-next-line @typescript-eslint/no-namespace -- TypeScript looks the JSX types up in a namespace named `JSX`
export namespace JSX {
  /** The value of every JSX expression. */
  export type Element = UiContent;
  /** The elements `<text>`, `<stack>`, `<date>` … with their props. */
  export type IntrinsicElements = CheckedIntrinsicElements;
  /** Children are passed as the `children` prop. */
  export interface ElementChildrenAttribute {
    children: unknown;
  }
  /** `key` on every element. */
  export type IntrinsicAttributes = KeyProp;
}
