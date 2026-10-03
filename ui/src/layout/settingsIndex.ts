/**
 * The settings page's search index (M5b, ADR 0109). The page's structure
 * comes from the layout (`settings-page { group … { section … { <widgets> }
 * } }` → `ResolvedLayout.settings`); what each settings widget contains
 * comes from the features: `Feature.settings` maps a widget kind to the
 * Fluent ids of every setting label it shows (`SettingsWidgetInfo`), merged
 * by `features/registry.ts` (`appRegistry.settings`). The index holds, per
 * section, the group label, the section label and keywords, and the labels
 * of every widget in the section's tree, so a search finds the section
 * that holds a setting. Pure; the settings page (WP163) renders it.
 */
import type { LayoutNode, ResolvedLayout } from "../api/generated";

/** What a feature declares about one settings widget kind. */
export interface SettingsWidgetInfo {
  /** Fluent ids of every setting label the widget shows. */
  labels: readonly string[];
  /** Fluent id of extra search words (space separated), if any. */
  keywords?: string;
}

/** Where an index entry's text comes from. */
export type SettingsEntryKind = "group" | "section" | "keywords" | "setting";

/** One searchable text. */
export interface SettingsIndexEntry {
  /** The section it leads to. */
  section: string;
  /** The section's group. */
  group: string;
  /** What the text is. */
  kind: SettingsEntryKind;
  /** The text as shown (translated). */
  text: string;
  /** The widget kind of a `setting` entry. */
  widget?: string;
}

/** A section with what matched, in page order. */
export interface SettingsSearchHit {
  section: string;
  group: string;
  /** The matching entries, best first (section label before settings). */
  matches: SettingsIndexEntry[];
}

function widgetKinds(node: LayoutNode, out: string[]): string[] {
  if (node.type === "widget") {
    if (!out.includes(node.kind)) out.push(node.kind);
  } else {
    for (const child of node.children) widgetKinds(child, out);
  }
  return out;
}

/**
 * Builds the index. `label` translates a layout label (`@id` or literal),
 * `t` a Fluent id. Section and group labels default to
 * `@settings.section.<id>` / `@settings.group.<id>`.
 */
export function buildSettingsIndex(
  layout: ResolvedLayout,
  infos: Readonly<Record<string, SettingsWidgetInfo>>,
  label: (text: string) => string,
  t: (id: string) => string,
): SettingsIndexEntry[] {
  const out: SettingsIndexEntry[] = [];
  for (const group of layout.settings) {
    const groupText = label(group.label ?? `@settings.group.${group.id}`);
    for (const section of group.sections) {
      const base = { section: section.id, group: group.id };
      out.push({
        ...base,
        kind: "section",
        text: label(section.label ?? `@settings.section.${section.id}`),
      });
      out.push({ ...base, kind: "group", text: groupText });
      if (section.keywords !== null) {
        out.push({ ...base, kind: "keywords", text: label(section.keywords) });
      }
      for (const kind of widgetKinds(section.root, [])) {
        const info = Object.hasOwn(infos, kind) ? infos[kind] : undefined;
        if (info === undefined) continue;
        for (const id of info.labels) {
          out.push({ ...base, kind: "setting", text: t(id), widget: kind });
        }
        if (info.keywords !== undefined) {
          out.push({ ...base, kind: "keywords", text: t(info.keywords), widget: kind });
        }
      }
    }
  }
  return out;
}

/** Lowercased, without diacritics, for matching. */
export function foldText(text: string): string {
  return text
    .normalize("NFD")
    .replace(/\p{Mn}/gu, "")
    .toLowerCase();
}

const KIND_ORDER: Readonly<Record<SettingsEntryKind, number>> = {
  section: 0,
  setting: 1,
  keywords: 2,
  group: 3,
};

/**
 * Sections matching `query`: every word of the query must occur in one of
 * the section's entries (each word may match a different entry). Sections
 * keep page order; an empty query matches nothing.
 */
export function searchSettings(
  index: readonly SettingsIndexEntry[],
  query: string,
): SettingsSearchHit[] {
  const words = foldText(query)
    .split(/\s+/)
    .filter((w) => w.length > 0);
  if (words.length === 0) return [];
  const bySection = new Map<string, SettingsIndexEntry[]>();
  for (const entry of index) {
    const list = bySection.get(entry.section) ?? [];
    list.push(entry);
    bySection.set(entry.section, list);
  }
  const hits: SettingsSearchHit[] = [];
  for (const [section, entries] of bySection) {
    const folded = entries.map((e) => foldText(e.text));
    if (!words.every((w) => folded.some((text) => text.includes(w)))) continue;
    const matches = entries
      .filter((_e, i) => words.some((w) => folded[i]?.includes(w)))
      .sort((a, b) => KIND_ORDER[a.kind] - KIND_ORDER[b.kind]);
    hits.push({ section, group: entries[0]?.group ?? "", matches });
  }
  return hits;
}
