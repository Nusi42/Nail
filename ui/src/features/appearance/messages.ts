/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids starting with one of
 * `prefixes` must exist in the English catalogs (`appearance.ftl`,
 * `core-appearance.ftl`); ids built at runtime are listed by `runtimeIds`.
 */
import type { Density, ThemeKind, ThemeOverridesIssue } from "../../api/generated";
import { LUCIDE_ICONS } from "../../widgets/lucideIcons";
import { APPEARANCE_DIALOGS } from "./editorStore";
import { BUILTIN_THEMES } from "./model";
import { TOKEN_GROUPS, TOKEN_MODES } from "./tokens";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["appearance-", "theme-", "density-"];

/** Literals with those prefixes that are not message ids (part, widget names). */
export const notMessages: readonly string[] = [
  "appearance-settings",
  "theme-card",
  "theme-gallery",
  // M5d (ADR 0131): the theme editor's widget kind and an `EditIssue` value.
  "theme-editor",
  "theme-not-found",
  // M5d WP183: window kinds of the theme dialogs.
  "theme-new",
  "theme-rename",
  "theme-delete",
  // ... the dialogs' part and an `EditIssue` value.
  "theme-dialog",
  "theme-exists",
  // Lucide icon names (e.g. `layout-panel-left`) share prefixes with message ids.
  ...Object.keys(LUCIDE_ICONS),
];

const KINDS: readonly ThemeKind[] = ["light", "dark", "high-contrast"];
const DENSITIES: readonly Density[] = ["comfortable", "compact"];
const ISSUES: readonly ThemeOverridesIssue[] = [
  "font-size-range",
  "radius-range",
  "accent-invalid",
];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    ...BUILTIN_THEMES.map(([id]) => `theme-name-${id}`),
    ...KINDS.map((k) => `theme-kind-${k}`),
    ...DENSITIES.map((d) => `density-${d}`),
    ...ISSUES.map((i) => `theme-overrides-issue-${i}`),
    // M5d WP183: window titles, token groups, modes and value origins.
    ...APPEARANCE_DIALOGS.map((d) => `window-${d}`),
    ...TOKEN_GROUPS.map((g) => `theme-editor-group-${g}`),
    ...TOKEN_MODES.map((m) => `theme-editor-mode-${m}`),
    ...(["base", "extended", "own"] as const).map((o) => `theme-editor-origin-${o}`),
    "message-view-attachment-save-saved",
    "message-view-attachment-save-downloaded",
    "message-view-attachment-save-title",
    "message-view-attachment-save-unsupported",
    "message-view-attachment-save-failed",
  ];
}
