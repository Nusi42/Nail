/**
 * The mail widgets' own registrations (M5b WP162, contract ADR 0109,
 * decisions ADR 0111), merged by `../features/registry.ts` like a
 * feature slot:
 *
 * - the `list-header` widget (search with its key hint, title with counts,
 *   filter chips `nail_api::views::ListFilter` → `UiState.listFilter`);
 * - `list.filter "<filter>"` sets the filter chip;
 * - `folders.toggle-section "<section id>"` collapses / expands a section
 *   of the grouped folder pane (remembered in the core's UI state).
 *
 * The widgets that existed before M5b stay in `./index.ts`
 * (`builtinWidgets`). Rules shared with the core: `./folderPane.ts`
 * (sections), `../layout/overflow.ts` (toolbar "More").
 */
import type { ActionInfo } from "../actions/palette";
import type { Feature } from "../features/types";
import { toggleCollapsedSection } from "./folderPaneState";
import { ListHeader } from "./ListHeader";
import { isListFilter } from "./listFilter";
import { DELETE_PERMANENTLY_DIALOG, DeletePermanentlyDialog } from "./DeletePermanently";
import { FOLDER_SECTION_TOGGLE_ACTION, LIST_FILTER_ACTION } from "../api/generated";
export { FOLDER_SECTION_TOGGLE_ACTION, LIST_FILTER_ACTION } from "../api/generated";

export { ListHeader };

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  "list-header": ListHeader,
};

/** Action ids → handlers. */
export const actions: Feature["actions"] = {
  [LIST_FILTER_ACTION]: {
    run: (ctx, args) => {
      const filter = args[0];
      if (isListFilter(filter)) ctx.store.getState().setListFilter(filter);
    },
    enabled: (_ctx, args) => isListFilter(args[0]),
  },
  [FOLDER_SECTION_TOGGLE_ACTION]: {
    run: async (ctx, args) => {
      const id = args[0];
      if (typeof id === "string" && id !== "") {
        await toggleCollapsedSection(ctx.client, ctx.queryClient, id);
      }
    },
    enabled: (_ctx, args) => typeof args[0] === "string" && args[0] !== "",
  },
};

/**
 * Command-palette descriptions and icons of these actions. Both need an
 * argument, so the palette lists them only through layout aliases.
 */
export const actionInfo: Readonly<Record<string, ActionInfo>> = {
  [LIST_FILTER_ACTION]: {
    title: "list-action-filter",
    keywords: "list-action-filter-keywords",
    availability: ["list"],
    hidden: true,
    icon: "list-filter",
  },
  [FOLDER_SECTION_TOGGLE_ACTION]: {
    title: "folders-action-toggle-section",
    hidden: true,
    icon: "chevron-down",
  },
};

/** Dialog names → components. */
export const dialogs: NonNullable<Feature["dialogs"]> = {
  [DELETE_PERMANENTLY_DIALOG]: DeletePermanentlyDialog,
};

/** "Delete permanently?" is a confirmation: modal (M5c, ADR 0116). */
export const windows: NonNullable<Feature["windows"]> = {
  [DELETE_PERMANENTLY_DIALOG]: { confirmation: true },
};
