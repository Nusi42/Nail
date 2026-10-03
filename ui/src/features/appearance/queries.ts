/**
 * Theme list and selection through the `NailClient` (M5a: `ListThemes`,
 * `SetTheme`, `SetThemePreferences`; every answer is the whole
 * `Appearance`, which also arrives as `ThemeChanged`).
 */
import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import type { Appearance, ThemePreferences } from "../../api/generated";
import { queryKeys, useClient } from "../../state/queries";

/** Query keys of this feature. */
export const appearanceKeys = {
  all: ["appearance"] as const,
  themes: ["appearance", "themes"] as const,
};

/**
 * Stores a new `Appearance` (a `ThemeChanged` event or a request's answer)
 * and fetches the resolved theme again when the active theme, the density
 * or the overrides changed — the core also sends `ConfigChanged` then, but a
 * remote session that follows its own browser (M5b) gets its new theme only
 * from `GetConfig`.
 */
export function applyAppearance(queryClient: QueryClient, appearance: Appearance): void {
  const before = queryClient.getQueryData<Appearance>(appearanceKeys.themes);
  queryClient.setQueryData(appearanceKeys.themes, appearance);
  const switched =
    before === undefined ||
    before.active !== appearance.active ||
    before.settings.density !== appearance.settings.density ||
    JSON.stringify(before.settings.overrides) !== JSON.stringify(appearance.settings.overrides);
  if (switched) void queryClient.invalidateQueries({ queryKey: queryKeys.config });
}

/** The themes, the selection, the active theme and the OS state. */
export function useAppearance() {
  const client = useClient();
  return useQuery({
    queryKey: appearanceKeys.themes,
    queryFn: async (): Promise<Appearance> =>
      (await client.request({ type: "ListThemes" }, "Themes")).appearance,
    // `ThemeChanged` keeps it current.
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });
}

/** Picks a fixed theme (turns Follow Windows off). */
export function useSetTheme() {
  const client = useClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (themeId: string) =>
      (await client.request({ type: "SetTheme", theme_id: themeId }, "Themes")).appearance,
    onSuccess: (appearance) => queryClient.setQueryData(appearanceKeys.themes, appearance),
  });
}

/** Follow Windows, the light / dark pair, density and overrides. */
export function useSetThemePreferences() {
  const client = useClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (preferences: ThemePreferences) =>
      (await client.request({ type: "SetThemePreferences", preferences }, "Themes")).appearance,
    onSuccess: (appearance) => queryClient.setQueryData(appearanceKeys.themes, appearance),
  });
}

/** The preferences part of the current settings (to change one field). */
export function preferencesOf(appearance: Appearance): ThemePreferences {
  const s = appearance.settings;
  return {
    follow_system: s.follow_system,
    light: s.light,
    dark: s.dark,
    density: s.density,
    overrides: { ...s.overrides },
  };
}
