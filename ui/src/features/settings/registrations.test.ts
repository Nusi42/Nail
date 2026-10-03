/**
 * The settings search covers the whole page (M5b WP163): every settings
 * widget on the default page registers its labels (`Feature.settings`),
 * every registered id is an English message without placeholders, and a
 * few real searches find the section a user expects.
 */
import defaultLayoutJson from "../../../../crates/nail-api/fixtures/default-layout.json";
import { normalizeLayout } from "../../api/normalize";
import { shippedCatalogs } from "../../i18n/catalogs";
import { createLocalizer, resolveLabel } from "../../i18n/localizer";
import { buildSettingsIndex, searchSettings } from "../../layout/settingsIndex";
import { appRegistry } from "../registry";
import { BUILTIN_SETTINGS } from "./builtins";
import { pageSections, rankSettingsHits, widgetNodes } from "./model";

const layout = normalizeLayout(defaultLayoutJson);
const en = createLocalizer(shippedCatalogs, ["en"]);
const infos = appRegistry.settings;

describe("settings registrations (Feature.settings)", () => {
  it("cover every widget kind on the default settings page", () => {
    const kinds = new Set(
      pageSections(layout).flatMap(({ section }) => widgetNodes(section.root).map((n) => n.kind)),
    );
    expect(kinds.size).toBeGreaterThan(30);
    for (const kind of kinds) {
      expect(Object.keys(infos), kind).toContain(kind);
      expect(infos[kind]?.labels.length, kind).toBeGreaterThan(0);
    }
  });

  it("name only registered widget kinds", () => {
    for (const kind of Object.keys(infos)) {
      expect(Object.keys(appRegistry.widgets), kind).toContain(kind);
    }
  });

  it("include the built-in widgets' labels", () => {
    for (const [kind, info] of Object.entries(BUILTIN_SETTINGS)) {
      expect(infos[kind]).toBe(info);
    }
  });

  it("use English messages without placeholders", () => {
    for (const [kind, info] of Object.entries(infos)) {
      const ids = [...info.labels, ...(info.keywords === undefined ? [] : [info.keywords])];
      expect(new Set(info.labels).size, `${kind}: duplicate labels`).toBe(info.labels.length);
      for (const id of ids) {
        expect(en.has(id), `${kind}: ${id}`).toBe(true);
        expect(en.t(id), `${kind}: ${id}`).not.toMatch(/\{\s*\$/);
      }
    }
  });
});

describe("settings search over the shipped texts", () => {
  const index = buildSettingsIndex(
    layout,
    infos,
    (text) => resolveLabel(en, text),
    (id) => en.t(id),
  );
  const first = (query: string) =>
    rankSettingsHits(searchSettings(index, query), query)[0]?.section;

  it.each([
    ["spam", "mail"],
    ["junk threshold", "mail"],
    ["dark theme", "appearance"],
    ["density", "appearance"],
    ["accent colour", "appearance"],
    ["out of office", "server-rules"],
    ["vacation", "server-rules"],
    ["pgp", "keys"],
    ["smartcard", "keys"],
    ["encryption at rest", "privacy"],
    ["recovery key", "privacy"],
    ["passkey", "remote"],
    ["quiet hours", "notifications"],
    ["shortcuts", "keyboard"],
    ["toolbar", "layout"],
    ["signature", "accounts"],
    ["virtual accounts", "accounts"],
    ["backup", "data"],
    ["thunderbird", "data"],
    ["start with windows", "desktop"],
    ["update channel", "desktop"],
    ["mcp", "agents"],
    ["plug-ins", "plugins"],
  ])("“%s” finds %s first", (query, section) => {
    expect(first(query)).toBe(section);
  });

  it("finds a setting label as a setting entry of its widget", () => {
    const hit = searchSettings(index, "quiet hours")[0];
    expect(hit?.matches).toContainEqual(
      expect.objectContaining({
        kind: "setting",
        text: en.t("notification-quiet-title"),
        widget: "notification-settings",
      }),
    );
  });
});
