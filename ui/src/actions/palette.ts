/**
 * The command palette's model (M4c, ADR 0094): every action the app can run
 * — built-in and feature actions, the layout's aliases, plug-in commands —
 * with a title, search keywords, the layout's key bindings, whether it is
 * available in the current context, and its place among the recently used
 * ones (kept by the core: `GetRecentActions` / `RecordActionUsed`).
 *
 * Features describe their actions with `Feature.actionInfo` (merged by
 * `features/registry.ts`); the palette feature (`features/palette/`, WP124)
 * adds descriptions of the built-ins, renders the list, does the fuzzy
 * search and runs the chosen action. This module is pure.
 */
import type { ActionAvailability, JsonValue, RecentAction, ResolvedLayout } from "../api/generated";
import { labelMessageId } from "../i18n/localizer";

/** How a feature describes one of its actions for the palette. */
export interface ActionInfo {
  /** Fluent id of the title ("Reply to all"). */
  title: string;
  /** Fluent id of extra search words ("answer respond"), space separated. */
  keywords?: string;
  /**
   * Where the action makes sense; absent = anywhere (`global`). The palette
   * shows unavailable actions dimmed at the end (or hides them, WP124).
   */
  availability?: readonly ActionAvailability[];
  /** Not listed (actions that only make sense with arguments, internals). */
  hidden?: boolean;
}

/** Where an entry comes from. */
export type PaletteSource = "app" | "layout" | "plugin";

/** One palette row. */
export interface PaletteEntry {
  /** The action id to run. */
  id: string;
  /** Arguments to run it with (layout aliases carry their own). */
  args: readonly JsonValue[];
  /** Display title. */
  title: string;
  /** Lowercased search words: the title's words plus the keywords. */
  keywords: string[];
  /** Key sequences bound to it (`ctrl+shift+m`, `g i`), layout order. */
  shortcuts: string[];
  /** Whether it can run in the current context. */
  available: boolean;
  /** Position among the recent actions (0 = most recent), else `null`. */
  recentRank: number | null;
  /** Where it comes from. */
  source: PaletteSource;
}

/** What `buildPalette` needs. */
export interface PaletteInput {
  /** Every registered action id (built-ins, features, plug-in commands). */
  actionIds: readonly string[];
  /** Descriptions by action id. */
  infos: Readonly<Record<string, ActionInfo>>;
  /** Plug-in command ids (their titles come from `pluginTitles`). */
  pluginTitles?: Readonly<Record<string, string>>;
  /** The resolved layout (labels, aliases, key bindings). */
  layout: ResolvedLayout;
  /** The contexts that apply now (`global` is always added). */
  contexts: ReadonlySet<ActionAvailability>;
  /** Recently used actions, most recent first. */
  recent: readonly RecentAction[];
  /** Translation. */
  t: (id: string) => string;
  /** Whether the handler can run now (`ActionHandler.enabled`). */
  enabled?: (id: string) => boolean;
}

/** The key sequences the layout binds to `id` (bindings without arguments). */
export function shortcutsFor(layout: ResolvedLayout, id: string): string[] {
  const out: string[] = [];
  for (const binding of layout.keys) {
    if (binding.action === id && binding.args.length === 0 && !out.includes(binding.key)) {
      out.push(binding.key);
    }
  }
  return out;
}

function words(text: string): string[] {
  return text
    .toLowerCase()
    .split(/[\s,.;:/()-]+/)
    .filter((w) => w.length > 0);
}

function availableIn(info: ActionInfo | undefined, contexts: ReadonlySet<ActionAvailability>) {
  const wanted = info?.availability ?? ["global"];
  return wanted.some((a) => a === "global" || contexts.has(a));
}

/**
 * The palette's rows: recent actions first (most recent first), then the
 * rest by title. Hidden actions and actions without any title are left
 * out.
 */
export function buildPalette(input: PaletteInput): PaletteEntry[] {
  const { layout, t, recent } = input;
  const contexts = new Set(input.contexts);
  contexts.add("global");
  const rank = new Map(recent.map((r, i) => [r.action_id, i]));
  const entries: PaletteEntry[] = [];
  const seen = new Set<string>();

  const push = (entry: Omit<PaletteEntry, "keywords">, keywordText: string) => {
    if (entry.title.trim() === "" || seen.has(entry.id)) return;
    seen.add(entry.id);
    entries.push({
      ...entry,
      keywords: [...new Set([...words(entry.title), ...words(keywordText)])],
    });
  };

  for (const id of input.actionIds) {
    const info = input.infos[id];
    if (info?.hidden) continue;
    const plugin = input.pluginTitles?.[id];
    const label = layout.actions[id]?.label;
    const title =
      label != null
        ? label.startsWith("@")
          ? t(labelMessageId(label))
          : label
        : info
          ? t(info.title)
          : (plugin ?? "");
    const enabled = input.enabled ? input.enabled(id) : true;
    push(
      {
        id,
        args: [],
        title,
        shortcuts: shortcutsFor(layout, id),
        available: enabled && availableIn(info, contexts),
        recentRank: rank.get(id) ?? null,
        source: plugin !== undefined ? "plugin" : "app",
      },
      info?.keywords ? t(info.keywords) : "",
    );
  }
  // Layout aliases (`actions { my-thing "nav.goto-folder" … }`).
  for (const [id, def] of Object.entries(layout.actions)) {
    if (def.alias_of == null || def.label == null) continue;
    const target = input.infos[def.alias_of];
    push(
      {
        id,
        args: [],
        title: def.label.startsWith("@") ? t(labelMessageId(def.label)) : def.label,
        shortcuts: shortcutsFor(layout, id),
        // The alias itself: the dispatcher adds the alias's arguments
        // (`window.open "settings"` is only enabled with its window).
        available: (input.enabled ? input.enabled(id) : true) && availableIn(target, contexts),
        recentRank: rank.get(id) ?? null,
        source: "layout",
      },
      "",
    );
  }
  return entries.sort((a, b) => {
    if (a.recentRank !== null || b.recentRank !== null) {
      if (a.recentRank === null) return 1;
      if (b.recentRank === null) return -1;
      return a.recentRank - b.recentRank;
    }
    return a.title.localeCompare(b.title);
  });
}
