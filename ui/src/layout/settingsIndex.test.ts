/**
 * The settings search index (M5b): built from the default layout's
 * settings page and the labels features register per widget kind.
 */
import defaultLayoutJson from "../../../crates/nail-api/fixtures/default-layout.json";
import { normalizeLayout } from "../api/normalize";
import {
  buildSettingsIndex,
  foldText,
  searchSettings,
  type SettingsWidgetInfo,
} from "./settingsIndex";

const TEXTS: Record<string, string> = {
  "junk-enabled": "Filter junk automatically",
  "junk-keywords": "spam",
  "notify-sound": "Play a sound",
  "theme-accent": "Accent colour",
};

describe("settings index (M5b)", () => {
  const layout = normalizeLayout(defaultLayoutJson);
  const infos: Record<string, SettingsWidgetInfo> = {
    "junk-settings": { labels: ["junk-enabled"], keywords: "junk-keywords" },
    "notification-settings": { labels: ["notify-sound"] },
    "appearance-settings": { labels: ["theme-accent"] },
  };
  const label = (text: string) => (text.startsWith("@") ? `[${text.slice(1)}]` : text);
  const t = (id: string) => TEXTS[id] ?? id;
  const index = buildSettingsIndex(layout, infos, label, t);

  it("indexes every section with its group, labels and settings", () => {
    const sections = new Set(index.map((e) => e.section));
    const pageSections = layout.settings.flatMap((g) => g.sections.map((s) => s.id));
    expect([...sections]).toEqual(pageSections);
    expect(index).toContainEqual({
      section: "accounts",
      group: "general",
      kind: "section",
      text: "[settings.section.accounts]",
    });
    expect(index).toContainEqual({
      section: "mail",
      group: "general",
      kind: "setting",
      text: "Filter junk automatically",
      widget: "junk-settings",
    });
    expect(index).toContainEqual({
      section: "mail",
      group: "general",
      kind: "keywords",
      text: "spam",
      widget: "junk-settings",
    });
  });

  it("finds the section that holds a setting, every word anywhere", () => {
    expect(searchSettings(index, "")).toEqual([]);
    expect(searchSettings(index, "   ")).toEqual([]);
    const hits = searchSettings(index, "SOUND");
    expect(hits.map((h) => h.section)).toEqual(["notifications"]);
    expect(hits[0]?.matches[0]?.text).toBe("Play a sound");
    // Words may match different entries of a section (group + setting).
    expect(searchSettings(index, "look accent").map((h) => h.section)).toEqual(["appearance"]);
    expect(searchSettings(index, "spam").map((h) => h.section)).toEqual(["mail"]);
    expect(searchSettings(index, "nothing-like-this")).toEqual([]);
    // Section labels sort before settings within a hit.
    const accounts = searchSettings(index, "accounts");
    expect(accounts[0]?.matches[0]?.kind).toBe("section");
  });

  it("folds case and diacritics", () => {
    expect(foldText("Überprüfen É")).toBe("uberprufen e");
  });
});
