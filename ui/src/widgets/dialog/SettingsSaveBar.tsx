/**
 * The pinned "Unsaved changes" bar of a settings section (R6, ADR 0239):
 * the settings form of the shared footer. It shows only while a widget of
 * the shown section has unsaved changes (`./settingsDrafts.ts`): Save
 * saves them all (the bar stays while one fails; the widget shows why),
 * Discard drops them. Leaving the section or closing Settings with
 * unsaved changes asks with the shared prompt (the settings page's close
 * guard).
 *
 * Parts: `settings-save-bar`, `settings-save-bar.text`, then
 * `dialog.footer` (Save: `data-action="save-settings"`, Discard:
 * `data-action="discard-settings"`).
 */
import { useId } from "react";
import { useT } from "../../i18n/I18nProvider";
import { useUiStoreApi } from "../../state/uiStore";
import { DialogFooter } from "./DialogFooter";
import { discardSettingsDrafts, saveSettingsDrafts, useSettingsDrafts } from "./settingsDrafts";

export function SettingsSaveBar({ name }: { /** The section's name, localized. */ name: string }) {
  const t = useT();
  const id = useId();
  const ui = useUiStoreApi();
  const { dirty, canSave, saving } = useSettingsDrafts();
  if (!dirty) return null;
  return (
    <div data-part="settings-save-bar" role="group" aria-labelledby={`${id}-text`}>
      <p data-part="settings-save-bar.text" id={`${id}-text`}>
        {t("dialog-unsaved-bar", { name })}
      </p>
      <DialogFooter
        primary={{
          label: t("common-save"),
          action: "save-settings",
          disabled: !canSave,
          onClick: () => void saveSettingsDrafts(ui),
        }}
        cancel={{
          label: t("common-discard"),
          action: "discard-settings",
          disabled: saving,
          onClick: () => {
            discardSettingsDrafts(ui);
          },
        }}
        busy={saving}
      />
    </div>
  );
}
