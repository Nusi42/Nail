/**
 * Fills in defaults for resolved theme / layout JSON.
 *
 * The core always sends complete objects (ADR 0002: `Option` fields are
 * present and `null`), but the reference fixtures and hand-written test data
 * omit empty fields, as serde's `#[serde(default)]` allows. Normalizing once
 * at the boundary keeps the renderer free of `?? []` noise. This is not
 * validation: `nail-config` owns that.
 */
import type {
  ActionDef,
  AncestorSelector,
  Breakpoint,
  KeyBinding,
  LayoutNode,
  MenuItem,
  PartCondition,
  PartRule,
  ResolvedLayout,
  ResolvedTheme,
  SourceLocation,
  ToolbarItem,
} from "./generated";

type Loose = Record<string, unknown>;

function obj(value: unknown): Loose {
  return value !== null && typeof value === "object" && !Array.isArray(value)
    ? (value as Loose)
    : {};
}

function arr(value: unknown): unknown[] {
  return Array.isArray(value) ? value : [];
}

function str(value: unknown, fallback: string): string {
  return typeof value === "string" ? value : fallback;
}

function strOrNull(value: unknown): string | null {
  return typeof value === "string" ? value : null;
}

function numOrNull(value: unknown): number | null {
  return typeof value === "number" && Number.isFinite(value) ? value : null;
}

function mapValues<T>(value: unknown, f: (v: unknown) => T): Record<string, T> {
  const out: Record<string, T> = {};
  for (const [key, v] of Object.entries(obj(value))) {
    out[key] = f(v);
  }
  return out;
}

function stringMap(value: unknown): Record<string, string> {
  const out: Record<string, string> = {};
  for (const [key, v] of Object.entries(obj(value))) {
    if (typeof v === "string") {
      out[key] = v;
    }
  }
  return out;
}

function condition(value: unknown): PartCondition {
  const c = obj(value);
  return { key: str(c.key, ""), value: strOrNull(c.value) };
}

function ancestor(value: unknown): AncestorSelector {
  const a = obj(value);
  return {
    selector: str(a.selector, ""),
    states: arr(a.states).filter((s): s is string => typeof s === "string"),
    conditions: arr(a.conditions).map(condition),
  };
}

/** A source location, or `null` when absent or malformed. */
function sourceLocation(value: unknown): SourceLocation | null {
  const source = obj(value);
  return value && typeof source.file === "string"
    ? { file: source.file, line: Number(source.line) || 0, column: Number(source.column) || 0 }
    : null;
}

function partRule(value: unknown): PartRule {
  const p = obj(value);
  return {
    selector: str(p.selector, ""),
    states: arr(p.states).filter((s): s is string => typeof s === "string"),
    conditions: arr(p.conditions).map(condition),
    ancestors: arr(p.ancestors).map(ancestor),
    declarations: stringMap(p.declarations),
    source: sourceLocation(p.source),
  };
}

/** Completes a resolved theme. */
export function normalizeTheme(raw: unknown): ResolvedTheme {
  const t = obj(raw);
  const message = obj(t.message);
  const mode = t.mode;
  return {
    name: str(t.name, "default"),
    mode: mode === "light" || mode === "dark" || mode === "high-contrast" ? mode : "system",
    tokens: mapValues(t.tokens, stringMap),
    modes: mapValues(t.modes, (set) => mapValues(set, stringMap)),
    parts: arr(t.parts).map(partRule),
    icons: mapValues(t.icons, (icon) => {
      const i = obj(icon);
      return { path: str(i.path, ""), view_box: str(i.view_box, "0 0 24 24") };
    }),
    message: {
      font_family: str(message.font_family, "inherit"),
      text_font_family: str(message.text_font_family, "monospace"),
      font_size: str(message.font_size, "14px"),
      line_height: str(message.line_height, "1.5"),
      max_width: str(message.max_width, "none"),
      quote_colors: arr(message.quote_colors).filter((c): c is string => typeof c === "string"),
      html_dark_mode:
        message.html_dark_mode === "off" || message.html_dark_mode === "invert"
          ? message.html_dark_mode
          : "smart",
    },
    sounds: stringMap(t.sounds),
    density: t.density === "compact" ? "compact" : "comfortable",
    raw_css: arr(t.raw_css).map((r) => {
      const o = obj(r);
      return { selector: str(o.selector, ""), css: str(o.css, "") };
    }),
  };
}

function jsonArray(value: unknown): ActionDef["args"] {
  return arr(value) as ActionDef["args"];
}

/** Completes one layout node (recursively). */
export function normalizeNode(raw: unknown): LayoutNode {
  const n = obj(raw);
  const id = strOrNull(n.id);
  const label = strOrNull(n.label);
  const children = () => arr(n.children).map(normalizeNode);
  switch (n.type) {
    case "split":
      return {
        type: "split",
        id,
        label,
        orientation: n.orientation === "vertical" ? "vertical" : "horizontal",
        sizes: arr(n.sizes).map((s) => str(s, "1fr")),
        min_sizes: arr(n.min_sizes).map(strOrNull),
        max_sizes: arr(n.max_sizes).map(strOrNull),
        collapsible: arr(n.collapsible).map((c) => c === true),
        resizable: n.resizable !== false,
        children: children(),
      };
    case "stack":
      return {
        type: "stack",
        id,
        label,
        sizes: arr(n.sizes).map((s) => str(s, "1fr")),
        children: children(),
      };
    case "tabs":
      return { type: "tabs", id, label, active: numOrNull(n.active) ?? 0, children: children() };
    default:
      return {
        type: "widget",
        id,
        label,
        kind: str(n.kind, ""),
        options: obj(n.options) as Extract<LayoutNode, { type: "widget" }>["options"],
        rows: arr(n.rows).filter((r): r is string => typeof r === "string"),
        source: sourceLocation(n.source),
      };
  }
}

function toolbarItem(value: unknown): ToolbarItem {
  const i = obj(value);
  if (i.type === "spacer" || i.type === "separator") {
    return { type: i.type };
  }
  return {
    type: "action",
    action: str(i.action, ""),
    args: jsonArray(i.args),
    label: strOrNull(i.label),
    icon: strOrNull(i.icon),
  };
}

function menuItem(value: unknown): MenuItem {
  const i = obj(value);
  if (i.type === "separator") {
    return { type: "separator" };
  }
  if (i.type === "submenu") {
    return { type: "submenu", label: str(i.label, ""), items: arr(i.items).map(menuItem) };
  }
  return {
    type: "action",
    action: str(i.action, ""),
    args: jsonArray(i.args),
    label: strOrNull(i.label),
    icon: strOrNull(i.icon),
  };
}

/** Completes a resolved layout. */
export function normalizeLayout(raw: unknown): ResolvedLayout {
  const l = obj(raw);
  return {
    name: str(l.name, "default"),
    windows: mapValues(l.windows, (bps) =>
      arr(bps).map((bp): Breakpoint => {
        const b = obj(bp);
        return {
          min_width: numOrNull(b.min_width),
          max_width: numOrNull(b.max_width),
          root: normalizeNode(b.root),
        };
      }),
    ),
    actions: mapValues(l.actions, (a): ActionDef => {
      const d = obj(a);
      return {
        label: strOrNull(d.label),
        icon: strOrNull(d.icon),
        alias_of: strOrNull(d.alias_of),
        args: jsonArray(d.args),
      };
    }),
    keys: arr(l.keys).map((k): KeyBinding => {
      const b = obj(k);
      return {
        context: str(b.context, "global"),
        key: str(b.key, ""),
        action: str(b.action, ""),
        args: jsonArray(b.args),
      };
    }),
    toolbars: mapValues(l.toolbars, (items) => arr(items).map(toolbarItem)),
    menus: mapValues(l.menus, (items) => arr(items).map(menuItem)),
  };
}
