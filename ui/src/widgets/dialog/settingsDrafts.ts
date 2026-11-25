/**
 * Unsaved settings (R6, ADR 0239): nothing in a settings section applies
 * until Save. Each settings widget with changes registers a draft
 * (`useSettingsDraft`) while it is shown; the settings page shows one
 * `SettingsSaveBar` for the shown section while any draft is dirty, saves
 * or discards them all, and asks with the shared prompt before the section
 * is left or Settings is closed (`../../layout/closeGuard.ts`).
 *
 * Drafts are kept per draft scope, by a key unique in it (the widget's
 * node id or kind). The settings page uses the UI store's own scope; a
 * window that shows settings widgets of its own (the plug-in manager
 * window) wraps them in `SettingsDraftScope`, so each window instance has
 * its own drafts, bar and question (R8).
 */
import {
  createContext,
  createElement,
  useContext,
  useEffect,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";
import { useStore } from "zustand";
import { createStore, type StoreApi } from "zustand/vanilla";
import { useUiStoreApi, type UiStore, UiStoreMap } from "../../state/uiStore";

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

/** The drafts of one settings page or window (see the module comment). */
export class DraftScope {
  readonly store: StoreApi<DraftsState> = createStore<DraftsState>()(() => ({
    flags: {},
    saving: false,
  }));
  readonly actions = new Map<string, () => SettingsDraft>();
}

const all = new UiStoreMap<DraftScope>();

/** Where drafts live: a scope, or a UI store (its own scope, the settings page's). */
export type DraftOwner = UiStore | DraftScope;

function draftsOf(owner: DraftOwner): DraftScope {
  if (owner instanceof DraftScope) return owner;
  return all.obtain(owner, () => new DraftScope());
}

const DraftScopeContext = createContext<DraftScope | null>(null);

/** Gives the settings widgets inside their own drafts (one scope per mount). */
export function SettingsDraftScope({ children }: { children: ReactNode }) {
  const [scope] = useState(() => new DraftScope());
  return createElement(DraftScopeContext.Provider, { value: scope }, children);
}

/** The drafts the calling component belongs to. */
export function useDraftScope(): DraftScope {
  const ui = useUiStoreApi();
  return useContext(DraftScopeContext) ?? draftsOf(ui);
}

function setFlags(owner: DraftOwner, key: string, flags: DraftFlags | null): void {
  const { store } = draftsOf(owner);
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
export function settingsDirty(owner: DraftOwner): boolean {
  return anyDirty(draftsOf(owner).store.getState());
}

/** Whether every unsaved change can be saved. */
export function settingsCanSave(owner: DraftOwner): boolean {
  return allSavable(draftsOf(owner).store.getState());
}

/** Saves every dirty draft in turn; `false` at the first that fails (the rest stay unsaved). */
export async function saveSettingsDrafts(owner: DraftOwner): Promise<boolean> {
  const { store, actions } = draftsOf(owner);
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
export function discardSettingsDrafts(owner: DraftOwner): void {
  const { store, actions } = draftsOf(owner);
  for (const [key, flags] of Object.entries(store.getState().flags)) {
    if (flags.dirty) actions.get(key)?.().discard();
  }
}

/** The page's view of the drafts: dirty, savable, saving. */
export function useSettingsDrafts(): { dirty: boolean; canSave: boolean; saving: boolean } {
  const { store } = useDraftScope();
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
  const ui = useDraftScope();
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
