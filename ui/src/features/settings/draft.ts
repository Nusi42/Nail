/**
 * The settings widgets' side of "nothing applies until Save" (R6, ADR
 * 0239): a form keeps its edits apart from the saved values and registers
 * them with the page (`useSettingsDraft`), which shows the pinned save bar
 * and saves or discards every draft of the shown section. Widgets never
 * show a Save, Apply or Undo row of their own.
 *
 * - `useSettingsForm(saved, { save })`: the edited value (the saved one
 *   until something changes), `set`, `dirty`; Save calls `save` with the
 *   edited value — it throws when the write failed (the widget shows why,
 *   the bar stays) and resolves once `saved` shows the written value (set
 *   or refetch the query before returning). Discard drops the edits.
 * - `useLeaveDrafts()`: moving to another item inside a widget (another
 *   account, client or user) asks with the shared prompt while the page
 *   has unsaved changes, like leaving the section does.
 */
import { useId, useState } from "react";
import { errorMessageOf } from "../../api/client";
import type { LocalizedMessage } from "../../api/generated";
import { askToLeave, useCloseGuard, useGuardKey } from "../../layout/closeGuard";
import { useUiStoreApi } from "../../state/uiStore";
import { sameJson } from "../../text/json";
import {
  discardSettingsDrafts,
  saveSettingsDrafts,
  settingsCanSave,
  settingsDirty,
  useSettingsDraft,
} from "../../widgets/dialog";

export interface SettingsFormOptions<T> {
  /**
   * Writes `value`; throws (or resolves `false`, the widget showed why)
   * when it failed. Resolves when `saved` shows the written value.
   */
  save: (value: T) => Promise<unknown>;
  /** The edited value can be saved (it is valid); default: always. */
  canSave?: (value: T) => boolean;
  /** Whether two values are the same; default: equal as JSON. */
  equal?: (a: T, b: T) => boolean;
  /** Told when a save failed (the widget shows why). */
  onError?: (error: unknown) => void;
  /** Told after a save succeeded. */
  onSaved?: () => void;
}

export interface SettingsForm<T> {
  /** The edited value: the saved one until something changes. */
  value: T;
  /** Changes the edited value (nothing is written before the page's Save). */
  set: (next: T | ((current: T) => T)) => void;
  /** The edited value differs from the saved one. */
  dirty: boolean;
  /** Drops the edits (also what the bar's Discard does). */
  discard: () => void;
  /**
   * Counts the saves and discards: a key for sub-forms that keep text of
   * their own (typed lists), so they start again from the value.
   */
  revision: number;
}

/** A settings form whose edits wait for the page's Save (see the module comment). */
export function useSettingsForm<T>(saved: T, options: SettingsFormOptions<T>): SettingsForm<T> {
  const key = useId();
  const [edited, setEdited] = useState<{ value: T } | null>(null);
  const [revision, setRevision] = useState(0);
  const equal = options.equal ?? sameJson;
  const value = edited === null ? saved : edited.value;
  const dirty = edited !== null && !equal(edited.value, saved);
  const discard = () => {
    setEdited(null);
    setRevision((r) => r + 1);
  };
  useSettingsDraft(key, {
    dirty,
    canSave: options.canSave?.(value) ?? true,
    save: async () => {
      if (edited === null) return true;
      try {
        if ((await options.save(edited.value)) === false) return false;
      } catch (error) {
        options.onError?.(error);
        return false;
      }
      setEdited(null);
      setRevision((r) => r + 1);
      options.onSaved?.();
      return true;
    },
    discard,
  });
  const set = (next: T | ((current: T) => T)) => {
    setEdited((old) => {
      const base = old === null ? saved : old.value;
      return { value: next instanceof Function ? next(base) : next };
    });
  };
  return { value, set, dirty, discard, revision };
}

/** What a settings form tells about its last save. */
export interface FormNotice {
  /** The Fluent id of the last save's notice ("Saved."), until something changes. */
  notice: string | null;
  setNotice: (id: string | null) => void;
  /** The `useSettingsForm` options that show `savedId` after a save and the error after a failure. */
  outcome: (savedId: string) => Pick<SettingsFormOptions<never>, "onSaved" | "onError">;
  /** Shows why something failed. */
  failed: (error: unknown) => void;
  /** Forgets the notice and the failure (the user changed something). */
  clear: () => void;
  /** The form's `issues` plus the last failure, if any. */
  problems: (issues: readonly LocalizedMessage[]) => LocalizedMessage[];
}

/** The notice and the failure of a settings form's saves. */
export function useFormNotice(): FormNotice {
  const [notice, setNotice] = useState<string | null>(null);
  const [failure, setFailure] = useState<LocalizedMessage | null>(null);
  const failed = (error: unknown) => {
    setFailure(errorMessageOf(error));
  };
  return {
    notice,
    setNotice,
    outcome: (savedId) => ({
      onSaved: () => {
        setNotice(savedId);
      },
      onError: failed,
    }),
    failed,
    clear: () => {
      setNotice(null);
      setFailure(null);
    },
    problems: (issues) => (failure === null ? [...issues] : [...issues, failure]),
  };
}

/**
 * Moves on to another item inside a settings widget: at once without
 * unsaved changes on the page, else after the shared prompt's Save or
 * Don't save.
 */
export function useLeaveDrafts(): (proceed: () => void) => void {
  const ui = useUiStoreApi();
  const guard = useGuardKey();
  return (proceed) => {
    askToLeave(ui, guard, proceed);
  };
}

/**
 * The settings page's close guard (ADR 0239): closing Settings or its
 * window, or leaving the section, asks while a shown widget has unsaved
 * changes; Save saves every draft, Don't save discards them.
 */
export function useSettingsGuard(sectionName: string): void {
  const ui = useUiStoreApi();
  useCloseGuard({
    name: () => sectionName,
    isDirty: () => settingsDirty(ui),
    save: () => saveSettingsDrafts(ui),
    canSave: () => settingsCanSave(ui),
    discard: () => {
      discardSettingsDrafts(ui);
    },
  });
}
