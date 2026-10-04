/**
 * The mock core's theme editor requests (M5d WP183, contract ADR 0131):
 * `GetThemeDetails`, `EditTheme`, `DuplicateTheme`, `RenameTheme`,
 * `DeleteTheme`, answered like the core:
 *
 * - User themes live in `state.files` (`themes/<id>.kdl`); the user's own
 *   `theme.kdl` edits (`EditTheme` target `user`) in `state.user`. Built-in
 *   themes are refused where a user theme is needed
 *   (`edit-issue-not-editable`), unknown ones are `not_found`.
 * - `GetThemeDetails` lists every token in schema order (the base theme's,
 *   then those a theme adds) and every part rule, each with its origin
 *   (`base`, `extended`, `own`); its `revision` is the appearance scope's.
 * - Every write is one entry of the `appearance` undo history
 *   (`recordMockEdit`), checks `base_revision` (`conflict`), and announces
 *   itself with `ThemeChanged` (+ `ConfigChanged` when the resolved theme
 *   changed), like the core.
 */
import type {
  EditIssue,
  EditKind,
  PartRule,
  Request,
  Response,
  ThemeDetails,
  ThemePartRule,
  ThemePatch,
  ThemeTokenValue,
  ValueOrigin,
} from "../../api/generated";
import { checkMockBaseRevision, mockRevision, recordMockEdit } from "../../api/editHistoryMock";
import { defaultTheme } from "../../api/mockData";
import type { MockCore } from "../../api/mockMail";
import {
  DEFAULT_DARK_THEME,
  DEFAULT_LIGHT_THEME,
  THEME_NAME_MAX,
  isThemeId,
  themeIdFromName,
} from "./model";
import {
  appearanceSnapshot,
  changed,
  themeChain,
  themeKey,
  userThemeInfo,
  type AppearanceMockState,
  type MockThemeFile,
} from "./mock";
import { BUILTIN_THEME_FILES, type MockThemeEdits } from "./themes";
import { TOKEN_MODES, isPartSelector, isPropertyName } from "./tokens";

/** The requests answered here. */
export const THEME_EDIT_REQUESTS = [
  "GetThemeDetails",
  "EditTheme",
  "DuplicateTheme",
  "RenameTheme",
  "DeleteTheme",
] as const satisfies readonly Request["type"][];

export type ThemeEditRequest = Extract<Request, { type: (typeof THEME_EDIT_REQUESTS)[number] }>;

export function isThemeEditRequest(request: Request): request is ThemeEditRequest {
  return (THEME_EDIT_REQUESTS as readonly string[]).includes(request.type);
}

function error(
  code: Extract<Response, { type: "Error" }>["code"],
  issue: EditIssue,
  theme: string,
): Response {
  return {
    type: "Error",
    code,
    message: `${issue}: ${theme}`,
    localized: { id: `edit-issue-${issue}`, args: { theme } },
  };
}

/** The theme a request names, and whether it must be a user theme. */
function target(request: ThemeEditRequest): { id: string; userOnly: boolean } | null {
  switch (request.type) {
    case "GetThemeDetails":
    case "DuplicateTheme":
      return { id: request.theme_id, userOnly: false };
    case "RenameTheme":
    case "DeleteTheme":
      return { id: request.theme_id, userOnly: true };
    case "EditTheme":
      return request.target.kind === "theme"
        ? { id: request.target.theme_id, userOnly: true }
        : null;
  }
}

let pristine: ReturnType<typeof defaultTheme> | null = null;

/** `split[k="v"] split.divider:hover` from a resolved rule. */
export function selectorText(
  rule: Pick<PartRule, "selector" | "states" | "conditions" | "ancestors">,
): string {
  const step = (
    selector: string,
    states: readonly string[],
    conditions: PartRule["conditions"],
  ): string =>
    selector +
    conditions.map((c) => (c.value === null ? `[${c.key}]` : `[${c.key}="${c.value}"]`)).join("") +
    states.map((s) => `:${s}`).join("");
  return [
    ...rule.ancestors.map((a) => step(a.selector, a.states, a.conditions)),
    step(rule.selector, rule.states, rule.conditions),
  ].join(" ");
}

/** `ThemeDetails` of a theme the state knows. */
export function mockThemeDetails(core: MockCore, id: string): ThemeDetails | null {
  const state = core.data.features.appearance;
  const info = state.themes.find((t) => t.id === id);
  if (!info) return null;
  pristine ??= defaultTheme();
  const tokens = new Map<string, ThemeTokenValue>();
  for (const [category, set] of Object.entries(pristine.tokens)) {
    for (const [name, value] of Object.entries(set)) {
      const modes: Record<string, string> = {};
      for (const mode of TOKEN_MODES) {
        const v = pristine.modes[mode]?.[category]?.[name];
        if (v !== undefined) modes[mode] = v;
      }
      tokens.set(`${category}.${name}`, { category, name, value, modes, origin: "base" });
    }
  }
  const set = (
    category: string,
    name: string,
    origin: ValueOrigin,
    value: string | undefined,
    modes: Readonly<Record<string, string>>,
  ) => {
    const key = `${category}.${name}`;
    const before = tokens.get(key);
    const next: ThemeTokenValue = before
      ? { ...before, modes: { ...before.modes } }
      : { category, name, value: value ?? "", modes: {}, origin };
    if (value !== undefined) {
      // A value a theme sets replaces the base and drops its mode variants.
      next.value = value;
      next.modes = {};
    }
    Object.assign(next.modes, modes);
    next.origin = origin;
    tokens.set(key, next);
  };
  const chain = themeChain(state, id);
  const builtin = chain.builtin === null ? undefined : BUILTIN_THEME_FILES.get(chain.builtin);
  if (builtin) {
    const origin: ValueOrigin = builtin.id === id ? "own" : "extended";
    for (const [category, values] of Object.entries(builtin.tokens)) {
      for (const [name, value] of Object.entries(values)) set(category, name, origin, value, {});
    }
    for (const [mode, values] of Object.entries(builtin.variants)) {
      for (const [category, names] of Object.entries(values)) {
        for (const [name, value] of Object.entries(names)) {
          set(category, name, origin, undefined, { [mode]: value });
        }
      }
    }
  }
  const parts: ThemePartRule[] = pristine.parts.map((rule) => ({
    selector: selectorText(rule),
    properties: { ...rule.declarations },
    origin: "base",
  }));
  for (const file of chain.files) {
    const origin: ValueOrigin = file.id === id ? "own" : "extended";
    for (const token of file.tokens) {
      set(token.category, token.name, origin, token.value, token.modes);
    }
    for (const rule of file.parts) {
      parts.push({ selector: rule.selector, properties: { ...rule.properties }, origin });
    }
  }
  return {
    info: structuredClone(info),
    editable: info.source === "user",
    revision: mockRevision(core, "appearance"),
    tokens: [...tokens.values()],
    parts,
  };
}

/** A value that cannot leave its KDL string or CSS declaration. */
function safeValue(value: string): boolean {
  return value.trim() !== "" && !/[{};<>\\]/.test(value) && !/url\s*\(/i.test(value);
}

const KEBAB = /^[a-z][a-z0-9-]*$/;

/** `record` without `key`. */
function without(record: Record<string, string>, key: string): Record<string, string> {
  return Object.fromEntries(Object.entries(record).filter(([k]) => k !== key));
}

/** Applies `patches` to a copy of `edits`; the first refused patch's issue otherwise. */
export function applyThemePatches(
  edits: MockThemeEdits,
  patches: readonly ThemePatch[],
): MockThemeEdits | EditIssue {
  const out = structuredClone(edits);
  for (const patch of patches) {
    switch (patch.op) {
      case "set-token": {
        const mode = patch.mode;
        if (
          !KEBAB.test(patch.category) ||
          !KEBAB.test(patch.name) ||
          (mode !== null && !(TOKEN_MODES as readonly string[]).includes(mode)) ||
          (patch.value !== null && !safeValue(patch.value))
        ) {
          return "invalid-token";
        }
        let token = out.tokens.find((t) => t.category === patch.category && t.name === patch.name);
        if (patch.value === null) {
          if (!token) break;
          if (mode === null) delete token.value;
          else token.modes = without(token.modes, mode);
          if (token.value === undefined && Object.keys(token.modes).length === 0) {
            out.tokens = out.tokens.filter((t) => t !== token);
          }
          break;
        }
        if (!token) {
          token = { category: patch.category, name: patch.name, modes: {} };
          out.tokens.push(token);
        }
        if (mode === null) token.value = patch.value.trim();
        else token.modes[mode] = patch.value.trim();
        break;
      }
      case "set-part-property": {
        if (
          !isPartSelector(patch.selector) ||
          !isPropertyName(patch.property) ||
          (patch.value !== null && !safeValue(patch.value))
        ) {
          return "invalid-part";
        }
        const selector = patch.selector.trim();
        const property = patch.property.trim();
        let rule = out.parts.find((r) => r.selector === selector);
        if (patch.value === null) {
          if (!rule) break;
          rule.properties = without(rule.properties, property);
          if (Object.keys(rule.properties).length === 0) {
            out.parts = out.parts.filter((r) => r !== rule);
          }
          break;
        }
        if (!rule) {
          rule = { selector, properties: {} };
          out.parts.push(rule);
        }
        rule.properties[property] = patch.value.trim();
        break;
      }
      case "reset-part":
        if (!isPartSelector(patch.selector)) return "invalid-part";
        out.parts = out.parts.filter((r) => r.selector !== patch.selector.trim());
        break;
    }
  }
  return out;
}

function validName(name: string): string | null {
  const trimmed = name.trim();
  return trimmed === "" || trimmed.length > THEME_NAME_MAX ? null : trimmed;
}

/** Writes through `write` as one undo entry and announces it. */
function commit(
  core: MockCore,
  kind: EditKind,
  write: (state: AppearanceMockState) => void,
): Extract<Response, { type: "Themes" }> {
  const state = core.data.features.appearance;
  const before = themeKey(core);
  const undo = appearanceSnapshot(core);
  write(state);
  const answer = changed(core, state, before);
  recordMockEdit(core, "appearance", kind, undo);
  return answer;
}

/** Replaces a user theme's `ThemeInfo` after its file changed. */
function refreshInfo(state: AppearanceMockState, file: MockThemeFile, oldId = file.id): void {
  const info = userThemeInfo(state, file);
  const index = state.themes.findIndex((t) => t.id === oldId);
  if (index < 0) state.themes.push(info);
  else state.themes[index] = info;
  // Themes extending it preview with its colours.
  for (const other of Object.values(state.files)) {
    if (other.extends === file.id && other.id !== file.id) refreshInfo(state, other);
  }
}

function edit(core: MockCore, request: Extract<ThemeEditRequest, { type: "EditTheme" }>): Response {
  const conflict = checkMockBaseRevision(core, "appearance", request.base_revision);
  if (conflict) return conflict;
  const state = core.data.features.appearance;
  const t = request.target;
  const current: MockThemeEdits =
    t.kind === "user" ? state.user : (state.files[t.theme_id] ?? state.user);
  const next = applyThemePatches(current, request.patches);
  if (typeof next === "string") {
    return error("invalid_request", next, t.kind === "theme" ? t.theme_id : "theme.kdl");
  }
  return commit(core, "edit-theme", (s) => {
    if (t.kind === "user") {
      s.user = next;
      return;
    }
    const file = s.files[t.theme_id];
    if (!file) return;
    file.tokens = next.tokens;
    file.parts = next.parts;
    refreshInfo(s, file);
  });
}

function duplicate(
  core: MockCore,
  request: Extract<ThemeEditRequest, { type: "DuplicateTheme" }>,
): Response {
  const state = core.data.features.appearance;
  const name = validName(request.name);
  if (name === null) return error("invalid_request", "invalid-theme-name", request.name);
  const source = state.themes.find((th) => th.id === request.theme_id);
  if (!source) return error("not_found", "theme-not-found", request.theme_id);
  const base = themeIdFromName(name);
  let id = base;
  for (let n = 2; state.themes.some((th) => th.id === id); n++) id = `${base}-${String(n)}`;
  const original = state.files[source.id];
  const file: MockThemeFile = original
    ? { ...structuredClone(original), id, name }
    : { id, name, kind: source.kind, extends: source.id, tokens: [], parts: [] };
  const answer = commit(core, "duplicate-theme", (s) => {
    s.files[id] = file;
    refreshInfo(s, file);
  });
  return { type: "ThemeSaved", theme_id: id, appearance: answer.appearance };
}

function rename(
  core: MockCore,
  request: Extract<ThemeEditRequest, { type: "RenameTheme" }>,
): Response {
  const state = core.data.features.appearance;
  const name = validName(request.name);
  if (name === null) return error("invalid_request", "invalid-theme-name", request.name);
  const oldId = request.theme_id;
  const newId = request.new_id ?? oldId;
  if (!isThemeId(newId)) return error("invalid_request", "invalid-theme-name", newId);
  if (newId !== oldId && state.themes.some((th) => th.id === newId)) {
    return error("invalid_request", "theme-exists", newId);
  }
  const answer = commit(core, "rename-theme", (s) => {
    const file = s.files[oldId];
    if (!file) return;
    file.name = name;
    if (newId !== oldId) {
      file.id = newId;
      s.files = Object.fromEntries(
        Object.entries(s.files).map(([id, f]) => [id === oldId ? newId : id, f]),
      );
      for (const other of Object.values(s.files)) {
        if (other.extends === oldId) other.extends = newId;
      }
      // The selection follows the file.
      const follow = (v: string) => (v === oldId ? newId : v);
      s.settings = {
        ...s.settings,
        theme: follow(s.settings.theme),
        light: follow(s.settings.light),
        dark: follow(s.settings.dark),
      };
    }
    refreshInfo(s, file, oldId);
  });
  return { type: "ThemeSaved", theme_id: newId, appearance: answer.appearance };
}

function remove(
  core: MockCore,
  request: Extract<ThemeEditRequest, { type: "DeleteTheme" }>,
): Response {
  const id = request.theme_id;
  return commit(core, "delete-theme", (s) => {
    const kind = s.files[id]?.kind;
    s.files = Object.fromEntries(Object.entries(s.files).filter(([key]) => key !== id));
    s.themes = s.themes.filter((th) => th.id !== id);
    // A selection naming it falls back to the default theme of its kind.
    const fallback = kind === "dark" ? DEFAULT_DARK_THEME : DEFAULT_LIGHT_THEME;
    s.settings = {
      ...s.settings,
      theme: s.settings.theme === id ? fallback : s.settings.theme,
      light: s.settings.light === id ? DEFAULT_LIGHT_THEME : s.settings.light,
      dark: s.settings.dark === id ? DEFAULT_DARK_THEME : s.settings.dark,
    };
  });
}

export function answerThemeEdit(core: MockCore, request: ThemeEditRequest): Response {
  const named = target(request);
  if (named) {
    const theme = core.data.features.appearance.themes.find((t) => t.id === named.id);
    if (!theme) return error("not_found", "theme-not-found", named.id);
    if (named.userOnly && theme.source !== "user") {
      return error("invalid_request", "not-editable", named.id);
    }
  }
  switch (request.type) {
    case "GetThemeDetails": {
      const details = mockThemeDetails(core, request.theme_id);
      return details
        ? { type: "ThemeDetails", details }
        : error("not_found", "theme-not-found", request.theme_id);
    }
    case "EditTheme":
      return edit(core, request);
    case "DuplicateTheme":
      return duplicate(core, request);
    case "RenameTheme":
      return rename(core, request);
    case "DeleteTheme":
      return remove(core, request);
  }
}
