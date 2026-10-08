/**
 * Views behind the navigation rail (M5b, ADR 0109) — the UI side of
 * `nail_api::views` and the layout's `views` / `rail` sections: the
 * constants are generated, the rules are ports pinned by
 * `crates/nail-api/tests/ts_vectors.rs` (vectors run in `views.test.ts`).
 *
 * - A view shows a layout window (`ViewDef.window`) inside the `view-host`
 *   widget; without a `view-host` in the main window, `view.show` opens the
 *   view's window instead, so the actions work with any layout.
 * - The start view is `views default=`, else the first view.
 * - `view.next` / `view.previous` step through the rail's views (else the
 *   definition order) and wrap around.
 */
import {
  type LayoutNode,
  type RailItem,
  type ResolvedLayout,
  VIEW_HOST_WIDGET,
  type ViewDef,
} from "../api/generated";
export {
  BUILTIN_VIEWS,
  LIST_HEADER_WIDGET,
  NAV_RAIL_WIDGET,
  SETTINGS_GROUPS,
  SETTINGS_PAGE_WIDGET,
  SETTINGS_SEARCH_ACTION,
  SETTINGS_SECTION_ACTION,
  VIEW_HOST_WIDGET,
  VIEW_NEXT_ACTION,
  VIEW_PREVIOUS_ACTION,
  VIEW_SHOW_ACTION,
} from "../api/generated";

/** The view a view id names, if the layout defines it. */
export function viewDef(layout: ResolvedLayout, id: string): ViewDef | null {
  return Object.hasOwn(layout.views, id) ? (layout.views[id] ?? null) : null;
}

/** `ResolvedLayout::start_view`. */
export function startView(layout: ResolvedLayout): string | null {
  const preferred = layout.default_view;
  if (preferred !== null && viewDef(layout, preferred) !== null) return preferred;
  return Object.keys(layout.views)[0] ?? null;
}

/** The view to show: `active` when the layout defines it, else the start view. */
export function currentView(layout: ResolvedLayout, active: string | null): string | null {
  return active !== null && viewDef(layout, active) !== null ? active : startView(layout);
}

/** `view_of_window`: the view whose window is `window`. */
export function viewOfWindow(layout: ResolvedLayout, window: string): string | null {
  for (const [id, def] of Object.entries(layout.views)) {
    if (def.window === window) return id;
  }
  return null;
}

/** `step_view`: the view after (`1`) or before (`-1`) `current`, wrapping. */
export function stepView(layout: ResolvedLayout, current: string, step: 1 | -1): string | null {
  let order = layout.rail
    .filter((item): item is Extract<RailItem, { type: "view" }> => item.type === "view")
    .map((item) => item.view)
    .filter((view) => viewDef(layout, view) !== null);
  if (order.length === 0) order = Object.keys(layout.views);
  if (order.length === 0) return null;
  const at = order.indexOf(current);
  if (at < 0) return startView(layout);
  return order[(((at + step) % order.length) + order.length) % order.length] ?? null;
}

/** The view's label: its own, else `@view.<id>`. */
export function viewLabel(layout: ResolvedLayout, id: string): string {
  return viewDef(layout, id)?.label ?? `@view.${id}`;
}

/** Whether the tree uses a widget of `kind`. */
export function usesWidget(node: LayoutNode, kind: string): boolean {
  if (node.type === "widget") return node.kind === kind;
  return node.children.some((child) => usesWidget(child, kind));
}

/** Whether any breakpoint of `window` hosts views (a `view-host` widget). */
export function hostsViews(layout: ResolvedLayout, window = "main"): boolean {
  const breakpoints = Object.hasOwn(layout.windows, window) ? layout.windows[window] : undefined;
  return (breakpoints ?? []).some((bp) => usesWidget(bp.root, VIEW_HOST_WIDGET));
}
