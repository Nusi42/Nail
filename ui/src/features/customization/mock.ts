/**
 * The mock core's answers to the customization requests (WP24), used by
 * `MockTransport` (`../../api/mock.ts` routes exactly these requests here).
 * State lives in `data.features.customization`
 * ([`createCustomizationMockState`]).
 *
 * - Saved searches work like in the core.
 * - `UpdateLayout` applies the patches to the mock's resolved layout and
 *   announces it with `ConfigChanged` (the real core writes `layout.kdl`).
 * - The merge requests understand a small subset of KDL, enough for demos
 *   and tests: the file's name, `tokens { <category> "<name>" "<value>" }`
 *   lines of a theme and `keys context="…" { bind "<key>" "<action>" }`
 *   lines of a layout. Everything else is ignored.
 * - Backups keep the previous resolved configuration; `RevertConfig`
 *   restores it. `OpenConfigFile` records the request.
 */
import type {
  ConfigBackup,
  ConfigFile,
  ConfigIssue,
  ConfigMergePreview,
  KeyBinding,
  LayoutNode,
  LayoutPatch,
  MergeChoice,
  MergeItem,
  MergeSectionPreview,
  MergeTake,
  Request,
  ResolvedLayout,
  ResolvedTheme,
  Response,
  SavedSearch,
} from "../../api/generated";
import defaultLayoutJson from "../../../../crates/nail-api/fixtures/default-layout.json";
import { errorResponse, type MockCore } from "../../api/mockMail";
import { normalizeLayout } from "../../api/normalize";
import { parseQuery } from "../../api/mockSearch";

/** The requests this feature answers. */
export const CUSTOMIZATION_REQUESTS = [
  "UpdateLayout",
  "PreviewConfigMerge",
  "ResolveConfigMerge",
  "ApplyConfigMerge",
  "ListConfigBackups",
  "RevertConfig",
  "OpenConfigFile",
  "ListSavedSearches",
  "SaveSavedSearch",
  "DeleteSavedSearch",
] as const satisfies readonly Request["type"][];

export type CustomizationRequest = Extract<
  Request,
  { type: (typeof CUSTOMIZATION_REQUESTS)[number] }
>;

export function isCustomizationRequest(request: Request): request is CustomizationRequest {
  return (CUSTOMIZATION_REQUESTS as readonly string[]).includes(request.type);
}

/** A backup of the mock configuration. */
export interface MockBackup {
  backup: ConfigBackup;
  theme: ResolvedTheme;
  layout: ResolvedLayout;
}

/** Mock-core state of this feature. */
export interface CustomizationMockState {
  savedSearches: SavedSearch[];
  /** Every `UpdateLayout` patch applied, in order. */
  layoutPatches: LayoutPatch[];
  /** `OpenConfigFile` requests, in order. */
  opened: { file: ConfigFile; line: number; column: number }[];
  /** Backups, newest first. */
  backups: MockBackup[];
  /** Answer `OpenConfigFile` with `unsupported` (web mode). */
  openUnsupported: boolean;
}

export function createCustomizationMockState(): CustomizationMockState {
  return {
    savedSearches: [],
    layoutPatches: [],
    opened: [],
    backups: [],
    openUnsupported: false,
  };
}

function state(core: MockCore): CustomizationMockState {
  return core.data.features.customization;
}

function saveSearch(core: MockCore, search: SavedSearch): Response {
  const error = parseQuery(search.query).error;
  if (error) return errorResponse("invalid_request", `the query does not parse: ${error.message}`);
  if (search.name.trim() === "") return errorResponse("invalid_request", "a name is required");
  const list = state(core).savedSearches;
  let saved: SavedSearch;
  if (search.id === null) {
    const position = list.reduce((max, s) => Math.max(max, s.position + 1), 0);
    saved = { ...search, id: core.nextId("search-"), position };
    list.push(saved);
  } else {
    const index = list.findIndex((s) => s.id === search.id);
    if (index < 0) return errorResponse("not_found", `saved search ${search.id}`);
    saved = { ...search };
    list[index] = saved;
  }
  list.sort((a, b) => a.position - b.position);
  core.emit({ type: "SavedSearchesChanged" });
  return { type: "SavedSearchSaved", search: { ...saved } };
}

function announce(core: MockCore): void {
  core.emit({
    type: "ConfigChanged",
    theme: structuredClone(core.data.theme),
    layout: structuredClone(core.data.layout),
  });
}

// --- UpdateLayout ------------------------------------------------------------

function nodeAt(root: LayoutNode, path: readonly number[]): LayoutNode | null {
  let node: LayoutNode | undefined = root;
  for (const index of path) {
    if (!node || node.type === "widget") return null;
    node = node.children[index];
  }
  return node ?? null;
}

/** The shipped default layout (what the reset patches go back to). */
function defaultLayout(): ResolvedLayout {
  return normalizeLayout(defaultLayoutJson);
}

/** Applies one patch to a layout; returns an error message or `null`. */
export function applyPatch(layout: ResolvedLayout, patch: LayoutPatch): string | null {
  switch (patch.op) {
    case "split-sizes":
    case "column-order": {
      const root = layout.windows[patch.node.window]?.[patch.node.breakpoint]?.root;
      const node = root ? nodeAt(root, patch.node.path) : null;
      if (!node) return `no node at ${JSON.stringify(patch.node)}`;
      if (patch.op === "split-sizes") {
        if (node.type !== "split" && node.type !== "stack") return "not a split or stack";
        if (patch.sizes.length !== node.children.length) return "wrong number of sizes";
        node.sizes = [...patch.sizes];
      } else {
        if (node.type !== "widget") return "not a list widget";
        node.options = { ...node.options, columns: patch.columns.join(" ") };
      }
      return null;
    }
    case "toolbar-order":
      layout.toolbars[patch.toolbar] = structuredClone(patch.items);
      return null;
    case "add-key-binding": {
      const binding: KeyBinding = {
        context: patch.context,
        key: patch.key.toLowerCase(),
        action: patch.action,
        args: [...patch.args],
      };
      const index = layout.keys.findIndex(
        (k) => k.context === binding.context && k.key === binding.key,
      );
      if (index >= 0) layout.keys[index] = binding;
      else layout.keys.push(binding);
      return null;
    }
    case "remove-key-binding": {
      const key = patch.key.toLowerCase();
      layout.keys = layout.keys.filter((k) => !(k.context === patch.context && k.key === key));
      return null;
    }
    // M2c: back to the default layout's version.
    case "reset-toolbar": {
      const items = defaultLayout().toolbars[patch.toolbar];
      layout.toolbars = Object.fromEntries(
        Object.entries(layout.toolbars).filter(([name]) => name !== patch.toolbar),
      );
      if (items) layout.toolbars[patch.toolbar] = items;
      return null;
    }
    case "reset-key-binding": {
      const key = patch.key.toLowerCase();
      const same = (k: KeyBinding) => k.context === patch.context && k.key === key;
      layout.keys = [...layout.keys.filter((k) => !same(k)), ...defaultLayout().keys.filter(same)];
      return null;
    }
  }
}

function updateLayout(core: MockCore, patches: readonly LayoutPatch[]): Response {
  const next = structuredClone(core.data.layout);
  for (const patch of patches) {
    const problem = applyPatch(next, patch);
    if (problem) return errorResponse("invalid_request", problem);
  }
  state(core).layoutPatches.push(...structuredClone(patches));
  core.data.layout = next;
  announce(core);
  return { type: "Done" };
}

// --- Merge (a tiny KDL subset) -----------------------------------------------

interface Incoming {
  name: string;
  /** `category.name` → value (themes). */
  tokens: Map<string, string>;
  /** `context key` → binding (layouts). */
  keys: Map<string, KeyBinding>;
}

/** Reads the supported subset of an incoming file; errors like the core's. */
export function readIncoming(
  file: ConfigFile,
  text: string,
): { incoming: Incoming } | { errors: ConfigIssue[] } {
  const root = file === "theme" ? "theme" : "layout";
  const lines = text.split(/\r?\n/);
  const first = lines.findIndex((l) => l.trim() !== "" && !l.trim().startsWith("//"));
  const head = new RegExp(`^\\s*${root}\\s+"([^"]*)"`).exec(lines[first] ?? "");
  if (!head) {
    return {
      errors: [
        {
          line: Math.max(first, 0) + 1,
          column: 1,
          message: `expected \`${root} "<name>" { … }\``,
        },
      ],
    };
  }
  const incoming: Incoming = { name: head[1] ?? "", tokens: new Map(), keys: new Map() };
  let context = "global";
  for (const line of lines) {
    const keys = /^\s*keys(?:\s+context="([^"]+)")?/.exec(line);
    if (keys) context = keys[1] ?? "global";
    const bind = /^\s*bind\s+"([^"]+)"\s+"([^"\s]+)"/.exec(line);
    if (bind && file === "layout") {
      const key = (bind[1] ?? "").toLowerCase();
      incoming.keys.set(`${context} ${key}`, {
        context,
        key,
        action: bind[2] ?? "",
        args: [],
      });
    }
    const token = /^\s*([a-z-]+)\s+"([a-z0-9-]+)"\s+"([^"]*)"\s*$/.exec(line);
    if (token && file === "theme" && token[1] !== "bind") {
      incoming.tokens.set(`${token[1]}.${token[2]}`, token[3] ?? "");
    }
  }
  return { incoming };
}

function item(
  id: string,
  key: string,
  mine: string | null,
  theirs: string | null,
  conflicts: MergeItem["conflicts"] = [],
): MergeItem {
  const change =
    mine === null
      ? "added"
      : theirs === null
        ? "removed"
        : mine === theirs
          ? "unchanged"
          : "changed";
  return { id, key, change, mine, theirs, conflicts };
}

function previewOf(core: MockCore, file: ConfigFile, incoming: Incoming): ConfigMergePreview {
  const sections: MergeSectionPreview[] = [];
  const mineName = file === "theme" ? core.data.theme.name : core.data.layout.name;
  if (mineName !== incoming.name) {
    sections.push({
      section: "header",
      items: [item("header:name", "name", `${file} "${mineName}"`, `${file} "${incoming.name}"`)],
    });
  }
  if (file === "theme") {
    const items: MergeItem[] = [];
    for (const [key, value] of incoming.tokens) {
      const [category, name] = key.split(".") as [string, string];
      const mine = core.data.theme.tokens[category]?.[name];
      items.push(
        item(
          `tokens:${key}`,
          key,
          mine === undefined ? null : `${category} "${name}" "${mine}"`,
          `${category} "${name}" "${value}"`,
        ),
      );
    }
    if (items.length > 0) sections.push({ section: "tokens", items });
  } else {
    const items: MergeItem[] = [];
    for (const binding of incoming.keys.values()) {
      const mine = core.data.layout.keys.find(
        (k) => k.context === binding.context && k.key === binding.key,
      );
      const text = (k: KeyBinding) => `bind "${k.key}" "${k.action}"`;
      const conflicts: MergeItem["conflicts"] =
        mine && mine.action !== binding.action
          ? [
              {
                kind: "duplicate-key",
                message: `your layout binds \`${mine.key}\` to \`${mine.action}\` in ${mine.context}`,
              },
            ]
          : [];
      items.push(
        item(
          `keys:${binding.context}: ${binding.key}`,
          `${binding.context}: ${binding.key}`,
          mine ? text(mine) : null,
          text(binding),
          conflicts,
        ),
      );
    }
    if (items.length > 0) sections.push({ section: "keys", items });
  }
  return { file, sections, errors: [] };
}

function resolve(
  core: MockCore,
  file: ConfigFile,
  text: string,
  choices: readonly MergeChoice[],
  fallback: MergeTake,
): { theme: ResolvedTheme; layout: ResolvedLayout } | Response {
  if (file !== "theme" && file !== "layout") {
    return errorResponse("invalid_request", `${file}.kdl cannot be merged`);
  }
  const read = readIncoming(file, text);
  if ("errors" in read) return errorResponse("invalid_request", "the incoming file has errors");
  const preview = previewOf(core, file, read.incoming);
  const ids = new Set(preview.sections.flatMap((s) => s.items.map((i) => i.id)));
  for (const choice of choices) {
    if (!ids.has(choice.item_id)) {
      return errorResponse("invalid_request", `unknown merge item \`${choice.item_id}\``);
    }
  }
  const takes = (id: string) =>
    (choices.find((c) => c.item_id === id)?.take ?? fallback) === "theirs";
  const theme = structuredClone(core.data.theme);
  const layout = structuredClone(core.data.layout);
  if (takes("header:name")) {
    if (file === "theme") theme.name = read.incoming.name;
    else layout.name = read.incoming.name;
  }
  for (const [key, value] of read.incoming.tokens) {
    if (!takes(`tokens:${key}`)) continue;
    const [category, name] = key.split(".") as [string, string];
    theme.tokens[category] = { ...theme.tokens[category], [name]: value };
  }
  for (const binding of read.incoming.keys.values()) {
    if (!takes(`keys:${binding.context}: ${binding.key}`)) continue;
    applyPatch(layout, { op: "add-key-binding", ...binding });
  }
  return { theme, layout };
}

function takeBackup(core: MockCore, file: ConfigFile): ConfigBackup {
  const backup: ConfigBackup = {
    id: core.nextId(`${file}-`),
    file,
    created_at: new Date().toISOString(),
  };
  state(core).backups.unshift({
    backup,
    theme: structuredClone(core.data.theme),
    layout: structuredClone(core.data.layout),
  });
  return backup;
}

// --- Dispatch ----------------------------------------------------------------

export function answerCustomization(core: MockCore, request: CustomizationRequest): Response {
  switch (request.type) {
    case "ListSavedSearches":
      return {
        type: "SavedSearches",
        searches: state(core).savedSearches.map((s) => ({ ...s })),
      };
    case "SaveSavedSearch":
      return saveSearch(core, request.search);
    case "DeleteSavedSearch": {
      const list = state(core).savedSearches;
      const index = list.findIndex((s) => s.id === request.search_id);
      if (index < 0) return errorResponse("not_found", `saved search ${request.search_id}`);
      list.splice(index, 1);
      core.emit({ type: "SavedSearchesChanged" });
      return { type: "Done" };
    }
    case "UpdateLayout":
      return updateLayout(core, request.patches);
    case "PreviewConfigMerge": {
      if (request.file !== "theme" && request.file !== "layout") {
        return errorResponse("invalid_request", `${request.file}.kdl cannot be merged`);
      }
      const read = readIncoming(request.file, request.incoming);
      const preview: ConfigMergePreview =
        "errors" in read
          ? { file: request.file, sections: [], errors: read.errors }
          : previewOf(core, request.file, read.incoming);
      return { type: "ConfigMergePreview", preview };
    }
    case "ResolveConfigMerge": {
      const result = resolve(
        core,
        request.file,
        request.incoming,
        request.choices,
        request.default,
      );
      if ("type" in result) return result;
      return {
        type: "ConfigMergeResolved",
        theme: request.file === "theme" ? result.theme : null,
        layout: request.file === "layout" ? result.layout : null,
      };
    }
    case "ApplyConfigMerge": {
      const result = resolve(
        core,
        request.file,
        request.incoming,
        request.choices,
        request.default,
      );
      if ("type" in result) return result;
      const backup = takeBackup(core, request.file);
      core.data.theme = result.theme;
      core.data.layout = result.layout;
      announce(core);
      return { type: "ConfigMergeApplied", backup };
    }
    case "ListConfigBackups":
      return {
        type: "ConfigBackups",
        backups: state(core)
          .backups.filter((b) => b.backup.file === request.file)
          .map((b) => ({ ...b.backup })),
      };
    case "RevertConfig": {
      const backups = state(core).backups;
      const target = backups.find(
        (b) =>
          b.backup.file === request.file &&
          (request.backup_id === null || b.backup.id === request.backup_id),
      );
      if (!target) return errorResponse("not_found", `no backup of ${request.file}.kdl`);
      const restoredTheme = structuredClone(target.theme);
      const restoredLayout = structuredClone(target.layout);
      takeBackup(core, request.file);
      if (request.file === "theme") core.data.theme = restoredTheme;
      else core.data.layout = restoredLayout;
      announce(core);
      return { type: "Done" };
    }
    case "OpenConfigFile":
      if (state(core).openUnsupported) {
        return errorResponse("unsupported", "opening files needs the desktop app");
      }
      state(core).opened.push({ file: request.file, line: request.line, column: request.column });
      return { type: "Done" };
  }
}
