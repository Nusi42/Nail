/**
 * The theme editor's requests through the `NailClient` (M5d, ADR 0131):
 * `GetThemeDetails`, `EditTheme`, `DuplicateTheme`, `RenameTheme`,
 * `DeleteTheme`, and making a theme the one in use. Every write answers
 * the whole `Appearance` (also pushed as `ThemeChanged`); the details are
 * fetched again after it, and on `EditHistoryChanged` of the appearance
 * scope (undo, redo, a change on disk).
 */
import { useMutation, useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import type { Appearance, ThemeDetails, ThemeEditTarget, ThemePatch } from "../../api/generated";
import { queryKeys, useClient } from "../../state/queries";
import { themeChangeFor } from "./model";
import { appearanceKeys, applyAppearance, preferencesOf } from "./queries";

/** Query keys of the editor. */
export const themeEditorKeys = {
  details: ["appearance", "details"] as const,
  detailsOf: (themeId: string) => ["appearance", "details", themeId] as const,
};

/** Fetches every theme's details again (after a write, undo or a change on disk). */
export function invalidateThemeDetails(queryClient: QueryClient): Promise<void> {
  return queryClient.invalidateQueries({ queryKey: themeEditorKeys.details });
}

/** One theme's tokens and part rules with their origins. */
export function useThemeDetails(themeId: string | null) {
  const client = useClient();
  return useQuery({
    queryKey: themeEditorKeys.detailsOf(themeId ?? ""),
    queryFn: async (): Promise<ThemeDetails> =>
      (await client.request({ type: "GetThemeDetails", theme_id: themeId ?? "" }, "ThemeDetails"))
        .details,
    enabled: themeId !== null,
    staleTime: Number.POSITIVE_INFINITY,
    retry: false,
  });
}

/** Stores a write's answer and refreshes what depends on it. */
async function written(queryClient: QueryClient, appearance: Appearance): Promise<void> {
  applyAppearance(queryClient, appearance);
  // The active theme's tokens may have changed without a switch.
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: queryKeys.config }),
    invalidateThemeDetails(queryClient),
  ]);
}

/** `EditTheme`: one write of several patches. */
export function useEditTheme() {
  const client = useClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (input: {
      target: ThemeEditTarget;
      patches: ThemePatch[];
      baseRevision?: string;
    }) => {
      const answer = await client.request(
        {
          type: "EditTheme",
          target: input.target,
          patches: input.patches,
          ...(input.baseRevision === undefined ? {} : { base_revision: input.baseRevision }),
        },
        "Themes",
      );
      await written(queryClient, answer.appearance);
      return answer.appearance;
    },
  });
}

/** `DuplicateTheme` ("New theme", "Duplicate as new theme"); resolves to the new id. */
export function useDuplicateTheme() {
  const client = useClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (input: { themeId: string; name: string }) => {
      const answer = await client.request(
        { type: "DuplicateTheme", theme_id: input.themeId, name: input.name },
        "ThemeSaved",
      );
      await written(queryClient, answer.appearance);
      return answer.theme_id;
    },
  });
}

/** `RenameTheme`; resolves to the (possibly new) id. */
export function useRenameTheme() {
  const client = useClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (input: { themeId: string; name: string; newId?: string }) => {
      const answer = await client.request(
        {
          type: "RenameTheme",
          theme_id: input.themeId,
          name: input.name,
          ...(input.newId === undefined ? {} : { new_id: input.newId }),
        },
        "ThemeSaved",
      );
      await written(queryClient, answer.appearance);
      return answer.theme_id;
    },
  });
}

/** `DeleteTheme`. */
export function useDeleteTheme() {
  const client = useClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (themeId: string) => {
      const answer = await client.request({ type: "DeleteTheme", theme_id: themeId }, "Themes");
      await written(queryClient, answer.appearance);
      return answer.appearance;
    },
  });
}

/**
 * Makes a theme the one in use (`themeChangeFor`): its half of the Follow
 * Windows pair, else the fixed theme.
 */
export function useUseTheme() {
  const client = useClient();
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (themeId: string) => {
      const appearance =
        queryClient.getQueryData<Appearance>(appearanceKeys.themes) ??
        (await client.request({ type: "ListThemes" }, "Themes")).appearance;
      const theme = appearance.themes.find((t) => t.id === themeId);
      if (!theme) return appearance;
      const change = themeChangeFor(appearance.settings, theme);
      const answer =
        change.kind === "set-theme"
          ? await client.request({ type: "SetTheme", theme_id: change.themeId }, "Themes")
          : await client.request(
              {
                type: "SetThemePreferences",
                preferences: { ...preferencesOf(appearance), [change.half]: themeId },
              },
              "Themes",
            );
      applyAppearance(queryClient, answer.appearance);
      return answer.appearance;
    },
  });
}
