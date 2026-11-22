/**
 * Unsaved changes of the rule editor (R2 design review, item 4; R6, ADR
 * 0239: the shared close guard and prompt, `layout/closeGuard.ts`).
 *
 * - **In a window** (the rules window, a popped-out one): the editor
 *   registers the window's close guard, so ×, Esc or another way of closing
 *   the window with unsaved changes asks "Save changes to “X”?" — Save /
 *   Don't save / Cancel (Discard changes / Keep editing while the rule has
 *   problems).
 * - **In Settings** (Mail & junk): the rule is a settings draft
 *   (`useSettingsDraft`), so the section's save bar and the settings page's
 *   own guard cover it (leaving the section or closing Settings asks).
 *
 * Either way, picking another rule in the list asks first with the same
 * prompt (`askToLeave`, through the rules store's `leave`).
 */
import { useEffect } from "react";
import { useStore } from "zustand";
import { askToLeave, useCloseGuard, useGuardKey } from "../../layout/closeGuard";
import { useUiStoreApi } from "../../state/uiStore";
import { useSettingsDraft } from "../../widgets/dialog";
// The store module itself: the settings barrel imports the feature registry.
import { settingsStoreFor } from "../settings/store";
import { useRulesUi } from "./store";

/** The settings draft key of the rule editor. */
export const RULES_SETTINGS_DRAFT = "rule-editor";

export interface RuleGuardProps {
  /** The rule's name as edited (empty: untitled). */
  name: string;
  dirty: boolean;
  /** The rule can be saved (no problems; its text reads). */
  canSave: boolean;
  /** Saves the rule: `true` when it was saved. */
  save: () => Promise<boolean>;
  /** Drops the changes: the editor shows the saved rule again. */
  revert: () => void;
}

function WindowGuard({ name, dirty, canSave, save, revert }: RuleGuardProps) {
  useCloseGuard({
    name: () => name,
    isDirty: () => dirty,
    canSave: () => canSave,
    save,
    discard: revert,
  });
  return null;
}

function SettingsDraft({ dirty, canSave, save, revert }: RuleGuardProps) {
  useSettingsDraft(RULES_SETTINGS_DRAFT, { dirty, canSave, save, discard: revert });
  return null;
}

/** Registers the editor's unsaved changes where it is shown. */
export function RuleGuard(props: RuleGuardProps) {
  const ui = useUiStoreApi();
  const key = useGuardKey();
  const settingsKey = useStore(settingsStoreFor(ui), (s) => s.guardKey);
  const setLeave = useRulesUi((s) => s.setLeave);
  useEffect(() => {
    setLeave((proceed) => {
      askToLeave(ui, key, proceed);
    });
    return () => {
      setLeave(null);
    };
  }, [ui, key, setLeave]);
  return settingsKey === key ? <SettingsDraft {...props} /> : <WindowGuard {...props} />;
}
