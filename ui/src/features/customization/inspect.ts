/**
 * What the inspector reports about an element (design.md, "Inspector"):
 * its part path, its states and data conditions, the theme rules that
 * apply to it (from the compiled theme) and the layout node that placed it,
 * with the file position to jump to.
 */
import type {
  ConfigFile,
  LayoutNode,
  PartCondition,
  PartRule,
  ResolvedLayout,
  ResolvedTheme,
  SourceLocation,
} from "../../api/generated";
import { selectBreakpointIndex } from "../../layout/engine";
import { elementSelector } from "../../theme/compile";

/** Parts of the inspector itself are never inspected. */
export const INSPECTOR_PART_PREFIX = "inspector";

/** States that are CSS pseudo-classes (see the theme compiler). */
const PSEUDO_STATES = new Set(["hover", "active", "focus", "focus-visible", "focus-within"]);

/** Attributes that are not data conditions of the theme. */
const NOT_CONDITIONS = new Set([
  "part",
  "state",
  "geom",
  "node-id",
  "key-context",
  "key-modal",
  "action",
  "window",
]);

/** The nearest element with a `data-part` (the element itself included). */
export function partElement(target: Element | null): Element | null {
  const el = target?.closest("[data-part]") ?? null;
  if (!el) return null;
  const part = el.getAttribute("data-part") ?? "";
  return part.startsWith(INSPECTOR_PART_PREFIX) ? null : el;
}

/** Part names from the outermost part down to the element. */
export function partPath(el: Element): string[] {
  const out: string[] = [];
  for (let cur: Element | null = el; cur; cur = cur.parentElement) {
    const part = cur.getAttribute("data-part");
    if (part) out.unshift(part);
  }
  return out;
}

/** The element's states (`data-state`, plus focus / hover when they hold). */
export function statesOf(el: Element): string[] {
  const states = (el.getAttribute("data-state") ?? "").split(/\s+/).filter(Boolean);
  if (el.ownerDocument.activeElement === el) states.push("focus");
  return states;
}

/** The element's data conditions (`data-tag="work"` → `tag="work"`). */
export function conditionsOf(el: Element): PartCondition[] {
  const out: PartCondition[] = [];
  for (const attr of Array.from(el.attributes)) {
    if (!attr.name.startsWith("data-")) continue;
    const key = attr.name.slice(5);
    if (NOT_CONDITIONS.has(key)) continue;
    out.push({ key, value: attr.value === "" ? null : attr.value });
  }
  return out;
}

/** A theme rule as written in `theme.kdl`, e.g. `message-list.row:unread .subject`. */
export function ruleText(
  rule: Pick<PartRule, "selector" | "states" | "conditions" | "ancestors">,
): string {
  const one = (selector: string, states: readonly string[], conditions: readonly PartCondition[]) =>
    selector +
    states.map((s) => `:${s}`).join("") +
    conditions.map((c) => (c.value === null ? `[${c.key}]` : `[${c.key}="${c.value}"]`)).join("");
  return [...rule.ancestors, rule]
    .map((part) => one(part.selector, part.states, part.conditions))
    .join(" ");
}

/** A theme rule that applies to the inspected element. */
export interface MatchedRule {
  text: string;
  declarations: Record<string, string>;
  source: SourceLocation | null;
  /** Pseudo states the rule needs (`hover`, `focus`, …): it applies only then. */
  when: string[];
}

function matchesIgnoringPseudo(el: Element, rule: PartRule): boolean {
  const plain = (states: readonly string[]) => states.filter((s) => !PSEUDO_STATES.has(s));
  try {
    const chain = rule.ancestors.map((a) =>
      elementSelector(a.selector, plain(a.states), a.conditions),
    );
    chain.push(elementSelector(rule.selector, plain(rule.states), rule.conditions));
    return el.matches(chain.join(" "));
  } catch {
    return false;
  }
}

/** Theme rules that style the element, in source order (later wins). */
export function matchingRules(el: Element, theme: ResolvedTheme): MatchedRule[] {
  return theme.parts
    .filter((rule) => matchesIgnoringPseudo(el, rule))
    .map((rule) => ({
      text: ruleText(rule),
      declarations: rule.declarations,
      source: rule.source,
      when: [...rule.ancestors.flatMap((a) => a.states), ...rule.states].filter((s) =>
        PSEUDO_STATES.has(s),
      ),
    }));
}

/** The layout node that placed an element. */
export interface LayoutNodeInfo {
  window: string;
  breakpoint: number;
  /** Child indexes from the breakpoint's root. */
  path: number[];
  node: LayoutNode;
  /** Kind of widget, or the container type. */
  kind: string;
  /** Where the node is written (widgets only), else `null`. */
  source: SourceLocation | null;
}

/** Finds a node by id in a tree, with its child-index path. */
export function findNodeById(
  root: LayoutNode,
  id: string,
): { node: LayoutNode; path: number[] } | null {
  if (root.id === id) return { node: root, path: [] };
  if (root.type === "widget") return null;
  for (const [index, child] of root.children.entries()) {
    const found = findNodeById(child, id);
    if (found) return { node: found.node, path: [index, ...found.path] };
  }
  return null;
}

/**
 * The layout node of the nearest element (the element itself included)
 * with a `data-node-id`, looked up in the window the element is in and the
 * breakpoint the window width selects.
 */
export function layoutNodeOf(
  el: Element,
  layout: ResolvedLayout,
  width: number,
): LayoutNodeInfo | null {
  const owner = el.closest("[data-node-id]");
  const id = owner?.getAttribute("data-node-id");
  if (!owner || !id) return null;
  const window = el.closest("[data-window]")?.getAttribute("data-window") ?? "main";
  const breakpoints = layout.windows[window] ?? [];
  const breakpoint = selectBreakpointIndex(breakpoints, width);
  const root = breakpoints[breakpoint]?.root;
  const found = root ? findNodeById(root, id) : null;
  if (!found) return null;
  const { node, path } = found;
  return {
    window,
    breakpoint,
    path,
    node,
    kind: node.type === "widget" ? node.kind : node.type,
    source: node.type === "widget" ? node.source : null,
  };
}

/** The config file a source location is in, when it is one of the user's files. */
export function configFileOf(file: string): ConfigFile | null {
  const name = file.replaceAll("\\", "/").split("/").pop() ?? "";
  const known: Record<string, ConfigFile> = {
    "theme.kdl": "theme",
    "layout.kdl": "layout",
    "accounts.kdl": "accounts",
    "rules.kdl": "rules",
  };
  // `defaults/theme.kdl` is the shipped default, not a file the user edits.
  if (file.replaceAll("\\", "/").startsWith("defaults/")) return null;
  return known[name] ?? null;
}
