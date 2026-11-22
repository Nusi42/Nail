/**
 * Unsaved settings (R6, ADR 0239): nothing in a settings section applies
 * until Save. Each settings widget with changes registers a draft
 * (`useSettingsDraft`) while it is shown; the settings page shows one
 * `SettingsSaveBar` for the shown section while any draft is dirty, saves
 * or discards them all, and asks with the shared prompt before the section
 * is left or Settings is closed (`../../layout/closeGuard.ts`).
 *
 * Drafts are kept per UI store, by a key unique on the page (the widget's
 * node id or kind).
 */
import { useEffect, useLayoutEffect, useRef } from "react";
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import { useUiStoreApi, type UiStore } from "../../state/uiStore";

/** What a settings widget tells the page about its unsaved changes. */
export interface SettingsDraft {
  /** It has changes that are not saved. */
  dirty: boolean;
  /** The changes can be saved (they are valid); default `true`. */
  canSave?: boolean;
  /** Saves the changes: `true` when saved, `false` when it failed (the widget shows why). */
  save: () => Promise<boolean> | boolean;
  /** Drops the changes: the widget shows the saved values again. */
  discard: () => void;
}

interface DraftFlags {
  dirty: boolean;
  canSave: boolean;
}

interface DraftsState {
  /** By key: the drafts of the shown widgets. */
  flags: Readonly<Record<string, DraftFlags>>;
  /** Save runs. */
  saving: boolean;
}

interface Drafts {
  store: StoreApi<DraftsState>;
  actions: Map<string, () => SettingsDraft>;
}

const all = new WeakMap<UiStore, Drafts>();

function draftsOf(ui: UiStore): Drafts {
  let drafts = all.get(ui);
  if (!drafts) {
    drafts = {
      store: createStore<DraftsState>()(() => ({ flags: {}, saving: false })),
      actions: new Map(),
    };
    all.set(ui, drafts);
  }
  return drafts;
}

function setFlags(ui: UiStore, key: string, flags: DraftFlags | null): void {
  const { store } = draftsOf(ui);
  const current = store.getState().flags[key];
  if (flags === null) {
    if (current === undefined) return;
    store.setState({
      flags: Object.fromEntries(Object.entries(store.getState().flags).filter(([k]) => k !== key)),
    });
    return;
  }
  if (current?.dirty === flags.dirty && current.canSave === flags.canSave) return;
  store.setState({ flags: { ...store.getState().flags, [key]: flags } });
}

/** Some draft has unsaved changes. */
const anyDirty = (s: DraftsState) => Object.values(s.flags).some((f) => f.dirty);
/** Every unsaved change can be saved. */
const allSavable = (s: DraftsState) => Object.values(s.flags).every((f) => !f.dirty || f.canSave);

/** Whether a shown settings widget has unsaved changes. */
export function settingsDirty(ui: UiStore): boolean {
  return anyDirty(draftsOf(ui).store.getState());
}

/** Whether every unsaved change can be saved. */
export function settingsCanSave(ui: UiStore): boolean {
  return allSavable(draftsOf(ui).store.getState());
}

/** Saves every dirty draft in turn; `false` at the first that fails (the rest stay unsaved). */
export async function saveSettingsDrafts(ui: UiStore): Promise<boolean> {
  const { store, actions } = draftsOf(ui);
  store.setState({ saving: true });
  try {
    for (const [key, flags] of Object.entries(store.getState().flags)) {
      if (!flags.dirty) continue;
      const draft = actions.get(key)?.();
      if (draft === undefined) continue;
      const saved = await Promise.resolve()
        .then(() => draft.save())
        .catch(() => false);
      if (!saved) return false;
    }
    return true;
  } finally {
    store.setState({ saving: false });
  }
}

/** Drops every unsaved change. */
export function discardSettingsDrafts(ui: UiStore): void {
  const { store, actions } = draftsOf(ui);
  for (const [key, flags] of Object.entries(store.getState().flags)) {
    if (flags.dirty) actions.get(key)?.().discard();
  }
}

/** The page's view of the drafts: dirty, savable, saving. */
export function useSettingsDrafts(): { dirty: boolean; canSave: boolean; saving: boolean } {
  const ui = useUiStoreApi();
  const { store } = draftsOf(ui);
  const dirty = useStore(store, anyDirty);
  const canSave = useStore(store, allSavable);
  const saving = useStore(store, (s) => s.saving);
  return { dirty, canSave, saving };
}

/**
 * Registers the unsaved changes of a settings widget under `key` while it
 * is mounted; the page's save bar saves or discards them.
 */
export function useSettingsDraft(key: string, draft: SettingsDraft): void {
  const ui = useUiStoreApi();
  const current = useRef(draft);
  useLayoutEffect(() => {
    current.current = draft;
  });
  useEffect(() => {
    const { actions } = draftsOf(ui);
    const read = () => current.current;
    actions.set(key, read);
    return () => {
      if (actions.get(key) === read) actions.delete(key);
      setFlags(ui, key, null);
    };
  }, [ui, key]);
  const dirty = draft.dirty;
  const canSave = draft.canSave ?? true;
  useEffect(() => {
    setFlags(ui, key, { dirty, canSave });
  }, [ui, key, dirty, canSave]);
}
