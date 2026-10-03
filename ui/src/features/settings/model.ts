/**
 * Pure rules of the settings page (M5b WP163): finding sections, resolving
 * deep links (`settings.section "<id>"` with a section id, a node id on the
 * page, a widget kind or the id of a former settings tab), and the match
 * ranges the search highlights.
 */
import type { LayoutNode, ResolvedLayout, SettingsSection } from "../../api/generated";
import { foldText, type SettingsSearchHit } from "../../layout/settingsIndex";
import formerTabs from "./formerTabs.json";

type WidgetNode = Extract<LayoutNode, { type: "widget" }>;

/**
 * Every node of the settings window's tabs before M5b (M5a's default
 * `window "settings"`), id → the widget kind that holds its settings
 * (`formerTabs.json`; data, not message ids). A container maps to the first
 * widget inside it — the Customize tab was a stack of the customize toolbar,
 * the configuration errors and the editors' tabs. These ids stay valid deep
 * links (`settings.section "security-keys"`), and the page test checks that
 * each still resolves to a section showing that widget — the checklist
 * that moving the tabs into sections lost no setting.
 */
export const FORMER_SETTINGS_TABS: Readonly<Record<string, string>> = formerTabs;

/** Every section of the page, in page order, with its group id. */
export function pageSections(
  layout: ResolvedLayout,
): { group: string; section: SettingsSection }[] {
  return layout.settings.flatMap((group) =>
    group.sections.map((section) => ({ group: group.id, section })),
  );
}

/** The section with `id`, if the page has it. */
export function findSection(layout: ResolvedLayout, id: string): SettingsSection | null {
  for (const group of layout.settings) {
    const section = group.sections.find((s) => s.id === id);
    if (section) return section;
  }
  return null;
}

/** Every widget node of a tree, in order. */
export function widgetNodes(node: LayoutNode, out: WidgetNode[] = []): WidgetNode[] {
  if (node.type === "widget") out.push(node);
  else for (const child of node.children) widgetNodes(child, out);
  return out;
}

/** Where a deep link leads: the section, and the widget in it if one was named. */
export interface SettingsTarget {
  section: string;
  /** The widget node's id (`data-node-id`), when the link named a widget. */
  node: string | null;
  /** The widget kind, when the link named a widget. */
  widget: string | null;
}

function findNode(
  layout: ResolvedLayout,
  match: (node: WidgetNode) => boolean,
): SettingsTarget | null {
  for (const { section } of pageSections(layout)) {
    const node = widgetNodes(section.root).find(match);
    if (node) return { section: section.id, node: node.id, widget: node.kind };
  }
  return null;
}

/**
 * Resolves a deep link: a section id; a node id on the page (or the former
 * tab's id, whose page node is `page-<id>`); the id of a former settings tab
 * (`FORMER_SETTINGS_TABS`); a widget kind. `null` when nothing matches.
 */
export function resolveSettingsTarget(layout: ResolvedLayout, id: string): SettingsTarget | null {
  if (id === "") return null;
  if (findSection(layout, id)) return { section: id, node: null, widget: null };
  const byId = findNode(layout, (n) => n.id === id || n.id === `page-${id}`);
  if (byId) return byId;
  const former = Object.hasOwn(FORMER_SETTINGS_TABS, id) ? FORMER_SETTINGS_TABS[id] : undefined;
  if (former !== undefined) {
    const byFormer = findNode(layout, (n) => n.kind === former);
    if (byFormer) return byFormer;
  }
  return findNode(layout, (n) => n.kind === id);
}

/** The section to show: the remembered one when the page has it, else the first. */
export function currentSection(layout: ResolvedLayout, remembered: unknown): string | null {
  if (typeof remembered === "string" && findSection(layout, remembered)) return remembered;
  return layout.settings[0]?.sections[0]?.id ?? null;
}

/** A piece of a highlighted text. */
export interface TextPiece {
  text: string;
  /** Whether a query word matched it. */
  match: boolean;
}

/**
 * Splits `text` into matched and unmatched pieces for `query`: every
 * occurrence of every query word, compared case- and diacritics-folded like
 * the search (`foldText`), mapped back onto the original characters.
 */
export function highlightText(text: string, query: string): TextPiece[] {
  const words = foldText(query)
    .split(/\s+/)
    .filter((w) => w.length > 0);
  if (words.length === 0 || text === "") return text === "" ? [] : [{ text, match: false }];
  // The folded text, and for each of its code units the original offset.
  const chars = Array.from(text);
  let folded = "";
  const origin: number[] = [];
  let offset = 0;
  for (const ch of chars) {
    const f = foldText(ch);
    for (let i = 0; i < f.length; i++) origin.push(offset);
    folded += f;
    offset += ch.length;
  }
  origin.push(offset);
  const marked = new Array<boolean>(text.length).fill(false);
  for (const word of words) {
    let at = folded.indexOf(word);
    while (at >= 0) {
      const start = origin[at] ?? 0;
      const endIndex = at + word.length;
      // The end of the last folded unit's original character.
      const lastStart = origin[endIndex - 1] ?? start;
      const end = Math.max(
        origin[endIndex] ?? text.length,
        lastStart + lastCharLength(text, lastStart),
      );
      for (let i = start; i < end && i < text.length; i++) marked[i] = true;
      at = folded.indexOf(word, at + 1);
    }
  }
  const pieces: TextPiece[] = [];
  for (let i = 0; i < text.length; i++) {
    const match = marked[i] ?? false;
    const last = pieces[pieces.length - 1];
    if (last && last.match === match) last.text += text[i];
    else pieces.push({ text: text[i] ?? "", match });
  }
  return pieces;
}

function lastCharLength(text: string, at: number): number {
  const code = text.codePointAt(at);
  return code !== undefined && code > 0xffff ? 2 : 1;
}

const RANK_KINDS = ["section", "setting", "keywords", "group"] as const;

/**
 * Orders search hits best first, keeping page order among equals: a
 * section whose label holds every query word, then one with a setting that
 * holds every word, then by the best kind of entry that matched at all.
 */
export function rankSettingsHits(
  hits: readonly SettingsSearchHit[],
  query: string,
): SettingsSearchHit[] {
  const words = foldText(query)
    .split(/\s+/)
    .filter((w) => w.length > 0);
  const score = (hit: SettingsSearchHit): number => {
    for (const [rank, kind] of RANK_KINDS.entries()) {
      const whole = hit.matches.some(
        (m) => m.kind === kind && words.every((w) => foldText(m.text).includes(w)),
      );
      if (whole) return rank;
    }
    const best = Math.min(...hit.matches.map((m) => RANK_KINDS.indexOf(m.kind)));
    return RANK_KINDS.length + (Number.isFinite(best) ? best : RANK_KINDS.length);
  };
  return hits
    .map((hit, index) => ({ hit, index, score: score(hit) }))
    .sort((a, b) => a.score - b.score || a.index - b.index)
    .map(({ hit }) => hit);
}
