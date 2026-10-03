import defaultLayoutJson from "../../../../crates/nail-api/fixtures/default-layout.json";
import type { Folder, ResolvedLayout } from "../../api/generated";
import { normalizeLayout } from "../../api/normalize";
import {
  BADGE_CAP,
  actionShortcuts,
  badgeLabelId,
  badgeSources,
  badgeText,
  canHost,
  inboxUnread,
  railEntries,
  railViews,
  rememberedView,
  viewKeyContext,
  viewShortcuts,
} from "./model";
import { hostLayout } from "./testing";

const base = (): ResolvedLayout => normalizeLayout(defaultLayoutJson);

function folder(role: Folder["role"], unread: number, id: string = role): Folder {
  return { id, account_id: "all", parent_id: null, name: id, path: id, role, unread, total: 99 };
}

describe("rail entries", () => {
  it("follows the default layout's rail", () => {
    const entries = railEntries(base());
    expect(entries.map((e) => e.type)).toEqual([
      "view",
      "view",
      "view",
      "view",
      "spacer",
      "action",
      "view",
    ]);
    expect(railViews(entries)).toEqual(["mail", "calendar", "contacts", "tasks", "settings"]);
    expect(badgeSources(entries)).toEqual(new Set(["unread", "confirmations"]));
  });

  it("drops unknown views and separators that separate nothing", () => {
    const layout: ResolvedLayout = {
      ...base(),
      rail: [
        { type: "separator" },
        { type: "view", view: "mail" },
        { type: "separator" },
        { type: "separator" },
        { type: "view", view: "ghost" },
        { type: "view", view: "tasks" },
        { type: "separator" },
        { type: "spacer" },
        { type: "separator" },
      ],
    };
    const entries = railEntries(layout);
    expect(entries.map((e) => (e.type === "view" ? e.id : e.type))).toEqual([
      "mail",
      "separator",
      "tasks",
      "spacer",
    ]);
    expect(badgeSources(entries)).toEqual(new Set(["unread"]));
  });
});

describe("shortcuts", () => {
  it("finds the aliases' bindings of a view, global ones first", () => {
    const layout = base();
    expect(viewShortcuts(layout, "mail")).toEqual(["ctrl+1", "g m"]);
    // The view shortcuts come first, then the older window keys (`g c`,
    // `g t`, `ctrl+,`), which show the view since WP165.
    expect(viewShortcuts(layout, "calendar")).toEqual(["ctrl+2", "g c"]);
    expect(viewShortcuts(layout, "tasks")).toEqual(["ctrl+4", "g t"]);
    expect(viewShortcuts(layout, "settings")).toEqual(["ctrl+,"]);
  });

  it("finds direct view.show bindings with the view as argument", () => {
    const layout: ResolvedLayout = {
      ...base(),
      keys: [
        { context: "list", key: "alt+s", action: "view.show", args: ["settings"] },
        { context: "global", key: "f9", action: "view.show", args: ["settings"] },
        { context: "global", key: "f8", action: "view.show", args: ["tasks"] },
        // An alias called with arguments is not "show settings".
        { context: "global", key: "f7", action: "view.settings", args: ["x"] },
      ],
    };
    expect(viewShortcuts(layout, "settings")).toEqual(["f9", "alt+s"]);
  });

  it("finds an action's bindings with the same arguments", () => {
    const layout: ResolvedLayout = {
      ...base(),
      keys: [
        { context: "global", key: "f2", action: "agents.open-center", args: [] },
        { context: "global", key: "f3", action: "agents.open-center", args: ["x"] },
      ],
    };
    expect(actionShortcuts(layout, "agents.open-center", [])).toEqual(["f2"]);
    expect(actionShortcuts(layout, "agents.open-center", ["x"])).toEqual(["f3"]);
    expect(actionShortcuts(layout, "palette.open", [])).toEqual([]);
  });
});

describe("views in the host", () => {
  it("gives the PIM views their keyboard context and the rest the global one", () => {
    const layout = base();
    const def = (id: string) => {
      const v = layout.views[id];
      if (v === undefined) throw new Error(id);
      return v;
    };
    expect(viewKeyContext("calendar", def("calendar"))).toBe("calendar");
    expect(viewKeyContext("contacts", def("contacts"))).toBe("contacts");
    expect(viewKeyContext("tasks", def("tasks"))).toBe("tasks");
    expect(viewKeyContext("mail", def("mail"))).toBe("global");
    expect(viewKeyContext("settings", def("settings"))).toBe("global");
    // A user view named differently over the calendar window.
    expect(viewKeyContext("agenda", { ...def("calendar"), label: null })).toBe("calendar");
  });

  it("refuses views whose window is missing or hosts views", () => {
    const layout = hostLayout();
    expect(canHost(layout, "mail")).toBe(true);
    expect(canHost(layout, "calendar")).toBe(true);
    expect(canHost(layout, "ghost")).toBe(false);
    const recursive: ResolvedLayout = {
      ...layout,
      views: {
        ...layout.views,
        loop: { label: null, icon: null, window: "main", badge: null },
        gone: { label: null, icon: null, window: "nowhere", badge: null },
      },
    };
    expect(canHost(recursive, "loop")).toBe(false);
    expect(canHost(recursive, "gone")).toBe(false);
  });

  it("remembers only views the layout defines", () => {
    const layout = base();
    expect(rememberedView(layout, "tasks")).toBe("tasks");
    expect(rememberedView(layout, "ghost")).toBeNull();
    expect(rememberedView(layout, 3)).toBeNull();
    expect(rememberedView(layout, null)).toBeNull();
  });
});

describe("badges", () => {
  it("counts unread mail in the inboxes only", () => {
    expect(inboxUnread(undefined)).toBe(0);
    expect(
      inboxUnread([folder("inbox", 4), folder("junk", 7), folder("inbox", 2, "inbox-2")]),
    ).toBe(6);
  });

  it("caps the visible number", () => {
    expect(badgeText(3)).toEqual({ id: "shell-rail-badge-count", args: { count: 3 } });
    expect(badgeText(BADGE_CAP)).toEqual({
      id: "shell-rail-badge-count",
      args: { count: BADGE_CAP },
    });
    expect(badgeText(BADGE_CAP + 1)).toEqual({
      id: "shell-rail-badge-many",
      args: { count: BADGE_CAP },
    });
    expect(badgeLabelId("unread")).toBe("shell-rail-item-unread");
  });
});
