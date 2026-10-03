/**
 * Test helpers of the shell: the default layout rearranged the way WP165
 * will ship it (main window = `nav-rail` + `view-host`, today's main
 * content in a `mail` window, the mail view showing it), so the shell's
 * tests run against the real windows, views and rail of the fixture.
 */
import defaultLayoutJson from "../../../../crates/nail-api/fixtures/default-layout.json";
import type { LayoutNode, ResolvedLayout } from "../../api/generated";
import { normalizeLayout, normalizeNode } from "../../api/normalize";

/** A widget node from a sparse description. */
function widgetNode(kind: string, options: Record<string, unknown> = {}): LayoutNode {
  return normalizeNode({ type: "widget", kind, id: kind, options });
}

/** The default layout with the rail and the view host in the main window. */
export function hostLayout(railOptions: Record<string, unknown> = {}): ResolvedLayout {
  const base = normalizeLayout(defaultLayoutJson);
  const main: LayoutNode = normalizeNode({
    type: "split",
    id: "shell",
    orientation: "horizontal",
    sizes: ["auto", "1fr"],
    resizable: false,
    children: [widgetNode("nav-rail", railOptions), widgetNode("view-host")],
  });
  const mailView = base.views["mail"];
  if (mailView === undefined) throw new Error("the default layout has no mail view");
  return {
    ...base,
    windows: {
      ...base.windows,
      main: [{ min_width: null, max_width: null, root: main }],
      mail: base.windows["main"] ?? [],
    },
    views: { ...base.views, mail: { ...mailView, window: "mail" } },
  };
}
