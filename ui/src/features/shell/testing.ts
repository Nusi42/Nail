/**
 * Test helpers of the shell: the default layout of the fixture (since WP165
 * its main window is `nav-rail` + `view-host` and the mail view shows the
 * `mail` window), so the shell's tests run against the real windows, views
 * and rail.
 */
import defaultLayoutJson from "../../../../crates/nail-api/fixtures/default-layout.json";
import type { JsonValue, LayoutNode, ResolvedLayout } from "../../api/generated";
import { normalizeLayout } from "../../api/normalize";

/** The default layout with `railOptions` on its `nav-rail` widget. */
export function hostLayout(railOptions: Record<string, JsonValue> = {}): ResolvedLayout {
  const base = normalizeLayout(defaultLayoutJson);
  const withOptions = (node: LayoutNode): LayoutNode =>
    node.type === "widget"
      ? node.kind === "nav-rail"
        ? { ...node, options: { ...node.options, ...railOptions } }
        : node
      : { ...node, children: node.children.map(withOptions) };
  return {
    ...base,
    windows: {
      ...base.windows,
      main: (base.windows.main ?? []).map((bp) => ({ ...bp, root: withOptions(bp.root) })),
    },
  };
}

/**
 * The default layout as a pre-M5b user layout sees it: its own main window
 * (the mail panes, no view host) and the mail view showing it — the
 * contract's fallback, where `view.show` opens a view's window.
 */
export function fallbackLayout(): ResolvedLayout {
  const base = normalizeLayout(defaultLayoutJson);
  const mailView = base.views.mail;
  if (mailView === undefined) throw new Error("the default layout has no mail view");
  const { mail, ...windows } = base.windows;
  return {
    ...base,
    windows: { ...windows, main: mail ?? [] },
    views: { ...base.views, mail: { ...mailView, window: "main" } },
  };
}
