/**
 * The visual-regression matrix (plan M5a WP154, contract ADR 0102): which
 * theme × density variants are captured, and why one is skipped.
 *
 * The theme list is data-driven: the core's `ListThemes` answer (probed
 * once by the runner, checked again by every worker) decides which themes
 * exist. The five built-in themes of the proposal are always part of the
 * matrix, so a theme the core does not list (yet) shows up as a skipped
 * variant with its reason instead of silently disappearing; any further
 * built-in theme the core lists is added after them.
 *
 * Pure and dependency-free (only erasable TypeScript), so `node --test`
 * runs its tests directly (`matrix.test.ts`).
 */
import { DENSITY_VALUES } from "../../../ui/src/api/generated/constants.ts";
import type { Density } from "../../../ui/src/api/generated/Density.ts";

/** The built-in themes of the M5 proposal, in gallery order. */
export const EXPECTED_THEMES = ["frost", "sage", "graphite", "nocturne", "high-contrast"] as const;

/**
 * A pseudo theme: whatever the server starts in, not switched. Only on
 * request (`NAIL_VISUAL_THEMES=current`): it lets the harness compare
 * looks before the core can switch themes, and drives its self-test
 * (`selftest.mjs`). Never part of the default matrix.
 */
export const CURRENT_THEME = "current";

/** `nail_api::appearance::Density` (generated). */
export const DENSITIES = DENSITY_VALUES;
export type { Density };

/** The parts of `nail_api::ThemeInfo` the harness reads. */
export interface ThemeInfoLike {
  id: string;
  kind: string;
  source: string;
  problem?: { id: string } | null;
}

/** The parts of `nail_api::Appearance` (the `Themes` answer) the harness reads. */
export interface AppearanceLike {
  themes: ThemeInfoLike[];
  settings: {
    theme: string;
    follow_system: boolean;
    light: string;
    dark: string;
    density: string;
    overrides: Record<string, unknown>;
  };
  active: string;
}

/**
 * What `ListThemes` answered: the appearance, or why there is none (an
 * `Error` answer such as `unsupported` / `m5a-not-ready` before WP152, or
 * a failure to start or reach the core).
 */
export type ThemeProbe = { ok: true; appearance: AppearanceLike } | { ok: false; reason: string };

/** One cell of the matrix. */
export interface Variant {
  /** `<theme>-<density>`: the baseline folder and the test title. */
  id: string;
  theme: string;
  density: Density;
  /** Set when the variant cannot be captured: the message shown for the skip. */
  skip?: string;
}

/** The id of a variant (also its baseline folder). */
export function variantId(theme: string, density: Density): string {
  return `${theme}-${density}`;
}

/** A core answer (`nail_api::Response`), loosely typed. */
export type CoreAnswer = { type: string } & Record<string, unknown>;

/** Turns a `ListThemes` answer into a probe result. */
export function probeFromAnswer(answer: CoreAnswer): ThemeProbe {
  if (answer.type === "Themes" && answer.appearance && typeof answer.appearance === "object") {
    return { ok: true, appearance: answer.appearance as AppearanceLike };
  }
  if (answer.type === "Error") {
    const localized = answer.localized as { id?: string } | undefined;
    const id = localized?.id ? ` / ${localized.id}` : "";
    return { ok: false, reason: `${String(answer.code)}${id}: ${String(answer.message)}` };
  }
  return { ok: false, reason: `unexpected answer ${answer.type}` };
}

/**
 * Why `theme` cannot be captured, or `null` when it can: `ListThemes`
 * failed, the theme is not listed, or it is listed with a problem.
 */
export function unavailableReason(probe: ThemeProbe, theme: string): string | null {
  if (theme === CURRENT_THEME) return null;
  if (!probe.ok) {
    return (
      `ListThemes is not available (${probe.reason}), so theme "${theme}" cannot be selected; ` +
      "skipped until the core implements theme selection (M5a WP152)"
    );
  }
  const info = probe.appearance.themes.find((t) => t.id === theme);
  if (!info) {
    const listed = probe.appearance.themes.map((t) => t.id).join(", ") || "none";
    return `theme "${theme}" is not listed by ListThemes (listed: ${listed}); skipped`;
  }
  if (info.problem) {
    return `theme "${theme}" is listed with the problem ${info.problem.id}; skipped`;
  }
  return null;
}

/** Splits a comma-separated filter (`NAIL_VISUAL_THEMES`, …); empty = no filter. */
export function parseFilter(value: string | undefined): string[] | null {
  const items = (value ?? "")
    .split(",")
    .map((s) => s.trim())
    .filter((s) => s.length > 0);
  return items.length > 0 ? items : null;
}

export interface PlanOptions {
  /** Only these themes (ids). */
  themes?: string[] | null;
  /** Only these densities. */
  densities?: string[] | null;
}

/**
 * The matrix: the expected built-ins in gallery order, then every other
 * built-in theme the core lists, each in both densities; user themes are
 * left out (their files are the user's, there are no baselines for them).
 */
export function planVariants(probe: ThemeProbe, options: PlanOptions = {}): Variant[] {
  const themes: string[] = [...EXPECTED_THEMES];
  if (probe.ok) {
    for (const t of probe.appearance.themes) {
      if (t.source === "builtin" && !themes.includes(t.id)) themes.push(t.id);
    }
  }
  const unknownDensities = (options.densities ?? []).filter(
    (d) => !(DENSITIES as readonly string[]).includes(d),
  );
  if (unknownDensities.length > 0) {
    throw new Error(
      `unknown density ${unknownDensities.join(", ")} (known: ${DENSITIES.join(", ")})`,
    );
  }
  if (options.themes?.includes(CURRENT_THEME)) themes.unshift(CURRENT_THEME);
  const out: Variant[] = [];
  for (const theme of themes) {
    if (options.themes && !options.themes.includes(theme)) continue;
    const skip = unavailableReason(probe, theme);
    for (const density of DENSITIES) {
      if (options.densities && !options.densities.includes(density)) continue;
      const variant: Variant = { id: variantId(theme, density), theme, density };
      const reason = skip ?? currentDensityReason(probe, theme, density);
      if (reason) variant.skip = reason;
      out.push(variant);
    }
  }
  return out;
}

/** The current theme in another density needs `SetThemePreferences`, hence `ListThemes`. */
function currentDensityReason(probe: ThemeProbe, theme: string, density: Density): string | null {
  if (theme !== CURRENT_THEME || density === "comfortable" || probe.ok) return null;
  return `ListThemes is not available (${probe.reason}), so density "${density}" cannot be selected; skipped`;
}

/**
 * The `SetThemePreferences` payload that selects `density` with Follow
 * Windows off, keeping the user's pair (it must stay a usable light and
 * dark theme) and dropping the overrides (baselines show the theme as
 * shipped).
 */
export function preferencesFor(appearance: AppearanceLike, density: Density) {
  return {
    follow_system: false,
    light: appearance.settings.light,
    dark: appearance.settings.dark,
    density,
    overrides: {},
  };
}
