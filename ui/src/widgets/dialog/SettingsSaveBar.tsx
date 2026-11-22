/**
 * The pinned "Unsaved changes" bar of a settings section (R6, ADR 0239):
 * the settings form of the shared footer. It shows only while a widget of
 * the shown section has unsaved changes (`./settingsDrafts.ts`): Save
 * saves them all (the bar stays while one fails; the widget shows why),
 * Discard drops them. Leaving the section or closing Settings with
 * unsaved changes asks with the shared prompt (the settings page's close
 * guard).
 *
 * Screen readers: a polite status (always in the page, visually hidden)
 * says when the section gets unsaved changes and when they were saved or
 * discarded. The focus: after Save or Discard pressed in the bar, the bar
 * goes and the focus moves to `focusAfter` (the section's title); a failed
 * Save gives it back to Save.
 *
 * Parts: `settings-save-bar.status` (the live status), `settings-save-bar`,
 * `settings-save-bar.text`, then `dialog.footer` (Save:
 * `data-action="save-settings"`, Discard: `data-action="discard-settings"`).
 */
import { useEffect, useId, useRef, useState, type RefObject } from "react";
import { useT } from "../../i18n/I18nProvider";
import { useUiStoreApi } from "../../state/uiStore";
import { DialogFooter } from "./DialogFooter";
import { discardSettingsDrafts, saveSettingsDrafts, useSettingsDrafts } from "./settingsDrafts";

/** What the bar did last (what the status says once the bar goes). */
type Pressed = "save" | "discard" | null;

export function SettingsSaveBar({
  name,
  focusAfter,
}: {
  /** The section's name, localized. */
  name: string;
  /** Takes the focus when the bar goes after its Save or Discard (the section's title). */
  focusAfter?: RefObject<HTMLElement | null>;
}) {
  const t = useT();
  const id = useId();
  const ui = useUiStoreApi();
  const { dirty, canSave, saving } = useSettingsDrafts();
  const bar = useRef<HTMLDivElement>(null);
  /** Save or Discard was pressed here: the focus moves once the bar goes. */
  const pressed = useRef<Pressed>(null);
  /** What the bar did last, for the status once it went. */
  const [outcome, setOutcome] = useState<Pressed>(null);
  const status = dirty
    ? t("dialog-unsaved-bar", { name })
    : outcome === null
      ? ""
      : t(outcome === "discard" ? "dialog-settings-discarded" : "dialog-settings-saved");
  // The pressed button went with the bar: the focus stays in the section.
  useEffect(() => {
    if (dirty || pressed.current === null) return;
    pressed.current = null;
    focusAfter?.current?.focus();
  }, [dirty, focusAfter]);
  // A failed Save: the bar stays, its Save (disabled while saving) gets the focus back.
  const [failures, setFailures] = useState(0);
  useEffect(() => {
    if (failures === 0 || saving) return;
    bar.current?.querySelector<HTMLButtonElement>('[data-action="save-settings"]')?.focus();
  }, [failures, saving]);
  return (
    <>
      <p data-part="settings-save-bar.status" aria-live="polite">
        {status}
      </p>
      {dirty && (
        <div ref={bar} data-part="settings-save-bar" role="group" aria-labelledby={`${id}-text`}>
          <p data-part="settings-save-bar.text" id={`${id}-text`}>
            {t("dialog-unsaved-bar", { name })}
          </p>
          <DialogFooter
            primary={{
              label: t("common-save"),
              action: "save-settings",
              disabled: !canSave,
              onClick: () => {
                pressed.current = "save";
                setOutcome("save");
                void saveSettingsDrafts(ui).then((ok) => {
                  if (ok) return;
                  pressed.current = null;
                  setFailures((n) => n + 1);
                });
              },
            }}
            cancel={{
              label: t("common-discard"),
              action: "discard-settings",
              disabled: saving,
              onClick: () => {
                pressed.current = "discard";
                setOutcome("discard");
                discardSettingsDrafts(ui);
              },
            }}
            busy={saving}
          />
        </div>
      )}
    </>
  );
}
