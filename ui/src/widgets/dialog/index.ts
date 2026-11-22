/**
 * The shared dialog pattern (R6, ADR 0239): one footer, one confirmation,
 * one unsaved-changes prompt and one settings save bar. Features build
 * their dialogs from these and never order buttons themselves; the close
 * guard lives in the window layer (`../../layout/closeGuard.ts`).
 */
export {
  ConfirmDialog,
  ConfirmGone,
  type ConfirmDialogProps,
  type ConfirmRisk,
} from "./ConfirmDialog";
export {
  DialogFooter,
  type DialogFooterProps,
  type FooterButton,
  type FooterCancel,
} from "./DialogFooter";
export { DialogForm, isMultilineSubmit } from "./DialogForm";
export { DialogOsContext, useDialogOs } from "./dialogOs";
export { footerOrder, osOfNavigator, viewerOs, type FooterRole } from "./order";
export { SettingsSaveBar } from "./SettingsSaveBar";
export {
  discardSettingsDrafts,
  saveSettingsDrafts,
  settingsCanSave,
  settingsDirty,
  useSettingsDraft,
  useSettingsDrafts,
  type SettingsDraft,
} from "./settingsDrafts";
export { UnsavedPrompt } from "./UnsavedPrompt";
export {
  askToLeave,
  closeWindowNow,
  confirmLeave,
  useCloseGuard,
  useGuardKey,
  type CloseGuard,
} from "../../layout/closeGuard";
export { askInWindow, pendingSlot, usePendingQuestion, type PendingSlot } from "./pending";
export { askQuestion, questionOf, QuestionWindow, type DialogQuestion } from "./QuestionWindow";
