/**
 * Views and the rail (M5b): the ports of `nail_api::views` (views, folder
 * pane sections, toolbar overflow) over the vectors generated from the Rust
 * rules (`views.json`), and the UI-only helpers.
 */
import type { LayoutNode, ResolvedLayout, ViewDef } from "../api/generated";
import defaultLayoutJson from "../../../crates/nail-api/fixtures/default-layout.json";
import viewsVectors from "../../../crates/nail-api/fixtures/ts-vectors/views.json";
import { normalizeLayout } from "../api/normalize";
import { defaultLayout, describeVectors } from "../test/vectors";
import { accountColorSlot, folderPaneSections } from "../widgets/folderPane";
import { overflowOrder } from "./overflow";
import {
  BUILTIN_VIEWS,
  SETTINGS_GROUPS,
  VIEW_HOST_WIDGET,
  currentView,
  hostsViews,
  startView,
  stepView,
  usesWidget,
  viewLabel,
  viewOfWindow,
} from "./views";

const view = (window: string): ViewDef => ({ label: null, icon: null, window, badge: null });

function layout(): ResolvedLayout {
  return normalizeLayout({
    name: "x",
    windows: {},
    views: { mail: view("mail"), calendar: view("calendar"), settings: view("settings") },
  });
}

describe("views (M5b)", () => {
  it("finds the start view and the view of a window", () => {
    const l = layout();
    expect(startView(l)).toBe("mail");
    expect(startView({ ...l, default_view: "calendar" })).toBe("calendar");
    expect(startView({ ...l, default_view: "nope" })).toBe("mail");
    expect(startView({ ...l, views: {} })).toBeNull();
    expect(currentView(l, "settings")).toBe("settings");
    expect(currentView(l, "ghost")).toBe("mail");
    expect(currentView(l, null)).toBe("mail");
    expect(viewOfWindow(l, "calendar")).toBe("calendar");
    expect(viewOfWindow(l, "composer")).toBeNull();
    expect(viewLabel(l, "mail")).toBe("@view.mail");
    expect(viewLabel({ ...l, views: { mail: { ...view("m"), label: "Post" } } }, "mail")).toBe(
      "Post",
    );
  });

  it("steps in rail order and wraps", () => {
    const l = layout();
    expect(stepView(l, "mail", 1)).toBe("calendar");
    expect(stepView(l, "mail", -1)).toBe("settings");
    expect(stepView(l, "nope", 1)).toBe("mail");
    const railed: ResolvedLayout = {
      ...l,
      rail: [
        { type: "view", view: "settings" },
        { type: "spacer" },
        { type: "view", view: "ghost" },
        { type: "view", view: "mail" },
      ],
    };
    expect(stepView(railed, "settings", 1)).toBe("mail");
    expect(stepView(railed, "mail", 1)).toBe("settings");
    expect(stepView({ ...l, views: {}, rail: [] }, "mail", 1)).toBeNull();
  });

  it("knows whether a window hosts views", () => {
    const host: LayoutNode = {
      type: "stack",
      id: null,
      label: null,
      sizes: [],
      children: [
        {
          type: "widget",
          id: null,
          label: null,
          kind: VIEW_HOST_WIDGET,
          options: {},
          rows: [],
          source: null,
        },
      ],
    };
    expect(usesWidget(host, VIEW_HOST_WIDGET)).toBe(true);
    const l = {
      ...layout(),
      windows: { main: [{ min_width: null, max_width: null, root: host }] },
    };
    expect(hostsViews(l)).toBe(true);
    expect(hostsViews(l, "other")).toBe(false);
  });

  it("matches the default layout's views, rail and settings groups", () => {
    const l = normalizeLayout(defaultLayoutJson);
    expect(Object.keys(l.views)).toEqual([...BUILTIN_VIEWS]);
    expect(startView(l)).toBe("mail");
    expect(l.views.mail?.badge).toBe("unread");
    expect(l.rail.some((item) => item.type === "action" && item.badge === "confirmations")).toBe(
      true,
    );
    expect(l.settings.map((g) => g.id)).toEqual([...SETTINGS_GROUPS]);
    // WP165: the main window hosts the views; the mail view shows `mail`.
    expect(hostsViews(l)).toBe(true);
    expect(l.views.mail?.window).toBe("mail");
    for (const toolbar of Object.keys(l.toolbars)) {
      expect(l.toolbar_settings[toolbar]).toBeDefined();
    }
  });
});

describeVectors("views", viewsVectors, {
  stepView: (_layout: string, current: string, step: 1 | -1) =>
    stepView(defaultLayout(), current, step),
  viewOfWindow: (_layout: string, window: string) => viewOfWindow(defaultLayout(), window),
  startView: () => startView(defaultLayout()),
  accountColorSlot,
  folderPaneSections,
  overflowOrder,
});
