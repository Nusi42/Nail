/**
 * The settings page's search index (M5b, ADR 0109). The page's structure
 * comes from the layout (`settings-page { group … { section … { <widgets> }
 * } }` → `ResolvedLayout.settings`); what each settings widget contains
 * comes from the features: `Feature.settings` maps a widget kind to the
 * Fluent ids of every setting label it shows (`SettingsWidgetInfo`), merged
 * by `features/registry.ts` (`appRegistry.settings`). The index holds, per
 * section, the group label, the section label and keywords, and the labels
 * of every widget in the section's tree, so a search finds the section
 * that holds a setting. Widgets and labels the platform lacks a feature for
 * (ADR 0202) are left out. Pure; the settings page (WP163) renders it.
 */
import type { LayoutNode, PlatformFeature, PlatformInfo, ResolvedLayout } from "../api/generated";
import { hasFeature } from "../api/platform";

/** What a feature declares about one settings widget kind. */
export interface SettingsWidgetInfo {
  /** Fluent ids of every setting label the widget shows. */
  labels: readonly string[];
  /** Fluent id of extra search words (space separated), if any. */
  keywords?: string;
  /** The platform feature the whole widget needs (ADR 0202); hidden without it. */
  requires?: PlatformFeature;
  /** Labels shown only with a platform feature (part of a widget). */
  platformLabels?: Readonly<Partial<Record<PlatformFeature, readonly string[]>>>;
}

/**
 * Whether `info`'s widget is shown on `platform` (`undefined` = every
 * widget, for callers that do not know it).
 */
export function widgetOffered(
  info: SettingsWidgetInfo | undefined,
  platform: PlatformInfo | undefined,
): boolean {
  return (
    info?.requires === undefined || platform === undefined || hasFeature(platform, info.requires)
  );
}

/** The labels of `info` shown on `platform` (`undefined` = all of them). */
function offeredLabels(info: SettingsWidgetInfo, platform: PlatformInfo | undefined): string[] {
  const labels = [...info.labels];
  for (const [feature, ids] of Object.entries(info.platformLabels ?? {})) {
    if (platform === undefined || hasFeature(platform, feature as PlatformFeature)) {
      labels.push(...ids);
    }
  }
  return labels;
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
 * `@settings.section.<id>` / `@settings.group.<id>`. `platform` leaves out
 * what the system lacks (`undefined` = keep everything).
 */
export function buildSettingsIndex(
  layout: ResolvedLayout,
  infos: Readonly<Record<string, SettingsWidgetInfo>>,
  label: (text: string) => string,
  t: (id: string) => string,
  platform?: PlatformInfo,
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
        if (info === undefined || !widgetOffered(info, platform)) continue;
        for (const id of offeredLabels(info, platform)) {
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
