/**
 * Questions of the settings sections (R6, ADR 0239): "Remove account…",
 * "Delete virtual account", "Reset the classifier", "Revoke" and the like
 * ask in the shared question window (`../../widgets/dialog/QuestionWindow.tsx`)
 * instead of an inline "Are you sure" row. A feature registers its kinds with
 * `SettingsConfirmDialog` (`dialogs`) and `{ confirmation: true }`
 * (`windows`), then asks with `askSettingsQuestion(ui, kind, question)`.
 */
export {
  askQuestion as askSettingsQuestion,
  questionOf as settingsQuestionOf,
  QuestionWindow as SettingsConfirmDialog,
  type DialogQuestion as SettingsQuestion,
} from "../../widgets/dialog";
