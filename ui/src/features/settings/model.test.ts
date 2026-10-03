/**
 * The settings page's pure rules (M5b WP163): deep links and the
 * checklist that every former settings tab still resolves to a section
 * showing its widget, the shown section, and search highlighting.
 */
import defaultLayoutJson from "../../../../crates/nail-api/fixtures/default-layout.json";
import type { LayoutNode, ResolvedLayout } from "../../api/generated";
import type { SettingsIndexEntry, SettingsSearchHit } from "../../layout/settingsIndex";
import { normalizeLayout } from "../../api/normalize";
import {
  FORMER_SETTINGS_TABS,
  currentSection,
  findSection,
  highlightText,
  pageSections,
  rankSettingsHits,
  resolveSettingsTarget,
  widgetNodes,
} from "./model";

const layout = normalizeLayout(defaultLayoutJson);

/** Every node of a tree (containers included). */
function allNodes(node: LayoutNode, out: LayoutNode[] = []): LayoutNode[] {
  out.push(node);
  if (node.type !== "widget") for (const child of node.children) allNodes(child, out);
  return out;
}

/** The widget kinds a section shows. */
function kindsOf(l: ResolvedLayout, id: string): string[] {
  const section = findSection(l, id);
  return section ? widgetNodes(section.root).map((n) => n.kind) : [];
}

describe("former settings tabs (the M5b move lost no setting)", () => {
  it.each(Object.entries(FORMER_SETTINGS_TABS))(
    "%s still opens a section showing %s",
    (id, kind) => {
      const target = resolveSettingsTarget(layout, id);
      expect(target).not.toBeNull();
      // `appearance` is also a section id: it opens the section as a whole.
      if (target?.widget !== null) expect(target?.widget).toBe(kind);
      expect(kindsOf(layout, target?.section ?? "")).toContain(kind);
    },
  );

  it("lists every node of the settings window's tabs while that window has them", () => {
    // M5a's `window "settings"` (WP165 replaces its tabs by the page; the
    // checklist above keeps the ids after that).
    const settingsWindow = layout.windows.settings?.[0]?.root;
    if (!settingsWindow || settingsWindow.type !== "tabs") return;
    const nodes = allNodes(settingsWindow).filter((n) => n !== settingsWindow);
    expect(nodes.length).toBeGreaterThan(30);
    for (const node of nodes) {
      expect(node.id, JSON.stringify(node).slice(0, 80)).not.toBeNull();
      const id = node.id ?? "";
      expect(Object.keys(FORMER_SETTINGS_TABS), id).toContain(id);
      if (node.type === "widget") expect(FORMER_SETTINGS_TABS[id]).toBe(node.kind);
    }
  });

  it("puts every widget kind of the old tabs on the page", () => {
    const pageKinds = new Set(
      pageSections(layout).flatMap(({ section }) => widgetNodes(section.root).map((n) => n.kind)),
    );
    for (const kind of new Set(Object.values(FORMER_SETTINGS_TABS))) {
      expect(pageKinds, kind).toContain(kind);
    }
  });
});

describe("resolveSettingsTarget", () => {
  it("resolves section ids, page node ids, widget kinds", () => {
    expect(resolveSettingsTarget(layout, "appearance")).toEqual({
      section: "appearance",
      node: null,
      widget: null,
    });
    expect(resolveSettingsTarget(layout, "page-junk-settings")).toEqual({
      section: "mail",
      node: "page-junk-settings",
      widget: "junk-settings",
    });
    // A former tab id whose page node is `page-<id>`.
    expect(resolveSettingsTarget(layout, "vacation")).toEqual({
      section: "server-rules",
      node: "page-vacation",
      widget: "vacation-editor",
    });
    expect(resolveSettingsTarget(layout, "remote-users")?.section).toBe("remote");
    // A widget kind that is no former tab id.
    expect(resolveSettingsTarget(layout, "column-editor")?.section).toBe("layout");
  });

  it("refuses what the page does not have", () => {
    expect(resolveSettingsTarget(layout, "")).toBeNull();
    expect(resolveSettingsTarget(layout, "no-such-thing")).toBeNull();
    const empty: ResolvedLayout = { ...layout, settings: [] };
    expect(resolveSettingsTarget(empty, "appearance")).toBeNull();
    expect(resolveSettingsTarget(empty, "security-keys")).toBeNull();
  });
});

describe("currentSection", () => {
  it("keeps a remembered section the page has, else the first", () => {
    expect(currentSection(layout, "privacy")).toBe("privacy");
    expect(currentSection(layout, "gone")).toBe("accounts");
    expect(currentSection(layout, 42)).toBe("accounts");
    expect(currentSection(layout, null)).toBe("accounts");
    expect(currentSection({ ...layout, settings: [] }, "privacy")).toBeNull();
  });
});

describe("highlightText", () => {
  const marked = (text: string, query: string) =>
    highlightText(text, query)
      .map((p) => (p.match ? `[${p.text}]` : p.text))
      .join("");

  it("marks every occurrence of every word, folded like the search", () => {
    expect(marked("Play a sound", "SOUND")).toBe("Play a [sound]");
    expect(marked("Junk mail settings", "mail junk")).toBe("[Junk] [mail] settings");
    expect(marked("Überprüfen", "uber")).toBe("[Über]prüfen");
    expect(marked("Crème brûlée", "brulee creme")).toBe("[Crème] [brûlée]");
    expect(marked("banana", "an")).toBe("b[anan]a");
    expect(marked("Keys 🔑 keys", "keys")).toBe("[Keys] 🔑 [keys]");
  });

  it("leaves text without matches whole", () => {
    expect(highlightText("Theme", "")).toEqual([{ text: "Theme", match: false }]);
    expect(highlightText("Theme", "  ")).toEqual([{ text: "Theme", match: false }]);
    expect(highlightText("Theme", "dark")).toEqual([{ text: "Theme", match: false }]);
    expect(highlightText("", "dark")).toEqual([]);
  });
});

describe("rankSettingsHits", () => {
  const entry = (section: string, kind: SettingsIndexEntry["kind"], text: string) =>
    ({ section, group: "g", kind, text }) as SettingsIndexEntry;
  const hit = (section: string, ...matches: SettingsIndexEntry[]): SettingsSearchHit => ({
    section,
    group: "g",
    matches,
  });

  it("puts a section label match first, then a whole setting match, keeping page order", () => {
    const hits = [
      hit("desktop", entry("desktop", "setting", "Allow plug-ins to load")),
      hit("other", entry("other", "keywords", "plug-ins extensions")),
      hit("plugins", entry("plugins", "section", "Plug-ins")),
      hit("split", entry("split", "setting", "plug"), entry("split", "group", "ins")),
    ];
    expect(rankSettingsHits(hits, "plug-ins").map((h) => h.section)).toEqual([
      "plugins",
      "desktop",
      "other",
      "split",
    ]);
    expect(rankSettingsHits([], "x")).toEqual([]);
  });
});
