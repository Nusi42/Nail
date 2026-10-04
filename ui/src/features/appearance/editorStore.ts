/**
 * What the theme editor window shows (M5d WP183): the theme and, after the
 * inspector's "Edit style", the part to jump to — kept per UI store (one
 * per page or pop-out) so the window, its dialogs (New theme, Rename,
 * Delete) and the opener agree, like `widgets/DeletePermanently.tsx` keeps
 * its pending messages.
 */
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import type { UiStore } from "../../state/uiStore";
import { useUiStoreApi } from "../../state/uiStore";

/** The theme editor's window kind (dialog name; also the widget kind). */
export const THEME_EDITOR_DIALOG = "theme-editor";
/** "New theme": name and base theme. */
export const THEME_NEW_DIALOG = "theme-new";
/** "Rename theme". */
export const THEME_RENAME_DIALOG = "theme-rename";
/** "Delete theme?" (a confirmation). */
export const THEME_DELETE_DIALOG = "theme-delete";

/** Every window kind of this feature. */
export const APPEARANCE_DIALOGS = [
  THEME_EDITOR_DIALOG,
  THEME_NEW_DIALOG,
  THEME_RENAME_DIALOG,
  THEME_DELETE_DIALOG,
] as const;

export interface ThemeEditorState {
  /** The theme the editor shows; `null` = the active theme. */
  themeId: string | null;
  /** The part selector to show in the Parts tab ("Edit style"), else `null`. */
  part: string | null;
  /** Counts jumps, so the editor reacts to the same part asked for again. */
  jump: number;
  /** The theme "New theme" starts from; `null` = the active theme. */
  newBase: string | null;
  /** The user theme the Rename / Delete windows act on. */
  subject: string | null;
  show: (themeId: string | null, part?: string | null) => void;
  setNewBase: (themeId: string | null) => void;
  setSubject: (themeId: string | null) => void;
}

const stores = new WeakMap<UiStore, StoreApi<ThemeEditorState>>();

/** The editor state of a UI store. */
export function themeEditorStoreFor(ui: UiStore): StoreApi<ThemeEditorState> {
  let store = stores.get(ui);
  if (!store) {
    store = createStore<ThemeEditorState>((set) => ({
      themeId: null,
      part: null,
      jump: 0,
      newBase: null,
      subject: null,
      show: (themeId, part = null) => {
        set((s) => ({ themeId, part, jump: s.jump + 1 }));
      },
      setNewBase: (newBase) => {
        set({ newBase });
      },
      setSubject: (subject) => {
        set({ subject });
      },
    }));
    stores.set(ui, store);
  }
  return store;
}

/** Reads the editor state of the current UI store. */
export function useThemeEditorState<T>(selector: (state: ThemeEditorState) => T): T {
  return useStore(themeEditorStoreFor(useUiStoreApi()), selector);
}

/**
 * Opens (or focuses) the theme editor on `themeId` (`null` = the active
 * theme), optionally on one part's style.
 */
export function openThemeEditor(
  ui: UiStore,
  themeId: string | null,
  part: string | null = null,
): void {
  themeEditorStoreFor(ui).getState().show(themeId, part);
  ui.getState().openWindow(THEME_EDITOR_DIALOG);
}

/** Opens "New theme", starting from `base` (`null` = the active theme). */
export function openNewTheme(ui: UiStore, base: string | null = null): void {
  themeEditorStoreFor(ui).getState().setNewBase(base);
  ui.getState().openWindow(THEME_NEW_DIALOG);
}

/** Opens "Rename theme" or "Delete theme?" for a user theme. */
export function openThemeAction(
  ui: UiStore,
  dialog: typeof THEME_RENAME_DIALOG | typeof THEME_DELETE_DIALOG,
  themeId: string,
): void {
  themeEditorStoreFor(ui).getState().setSubject(themeId);
  ui.getState().openWindow(dialog);
}
