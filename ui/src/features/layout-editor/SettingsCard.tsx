/**
 * Settings → Look & feel → Layout & toolbars: the layout card (widget
 * `layout-editor-settings`, M5d WP181). Pick a window (Mail, Calendar, …),
 * then "Customize layout" (shows the view and turns on customize mode),
 * choose one of its presets, or reset it to the default.
 *
 * A chosen preset is a settings draft (R8, ADR 0242): it shows as chosen
 * and is written with the settings bar's Save like every other setting
 * (Discard forgets it). "Reset to default" and the customize mode stay
 * actions of their own with Undo.
 */
import { useId, useState } from "react";
import { useT } from "../../i18n/I18nProvider";
import { useLayoutConfig } from "../../layout/LayoutContext";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { useWindowWidth } from "../../layout/useWindowWidth";
import { useUiStoreApi } from "../../state/uiStore";
import { Icon } from "../../widgets/Icon";
import { customizableWindows, presetActive, presetFits, presetsOf, shownBreakpoint } from "./model";
import { useLayoutEdit } from "./useLayoutEdit";
import { usePresetName, useWindowName } from "./names";
import { PresetPreview } from "./Palette";
import { startCustomizing } from "./start";
import { useEditor } from "./store";
import { useSettingsForm } from "../settings/draft";

export function LayoutEditorSettings({ node }: WidgetProps) {
  const t = useT();
  const id = useId();
  const layout = useLayoutConfig();
  const store = useUiStoreApi();
  const width = useWindowWidth();
  const windowName = useWindowName(layout);
  const presetName = usePresetName();
  const edit = useLayoutEdit();
  const busy = useEditor((s) => s.busy);
  const message = useEditor((s) => (s.window === null ? s.message : null));
  const windows = customizableWindows(layout);
  const [chosen, setChosen] = useState<string | null>(null);
  const window = chosen !== null && windows.includes(chosen) ? chosen : (windows[0] ?? null);
  const breakpoint = window === null ? -1 : shownBreakpoint(layout, window, width);
  const root = window === null ? null : (layout.windows[window]?.[breakpoint]?.root ?? null);
  const presets = window === null ? [] : presetsOf(layout, window);
  // The preset waiting for the bar's Save (`null`: none chosen), with where it goes.
  const chosenPreset = useSettingsForm<{
    preset: string;
    window: string;
    breakpoint: number;
  } | null>(null, {
    save: async (choice) => {
      if (choice === null) return true;
      const preset = presetsOf(layout, choice.window).find((p) => p.id === choice.preset);
      return edit(
        [
          {
            op: "apply-preset",
            preset: choice.preset,
            target: { window: choice.window, breakpoint: choice.breakpoint, path: [] },
          },
        ],
        {
          id: "layout-editor-preset-applied",
          args: { preset: preset ? presetName(preset) : choice.preset },
        },
      );
    },
  });
  const pending = chosenPreset.value?.window === window ? chosenPreset.value.preset : null;

  return (
    <section
      data-part="card"
      data-section="arrangement"
      data-node-id={node.id ?? undefined}
      aria-labelledby={`${id}-title`}
    >
      <h3 data-part="card.title" id={`${id}-title`}>
        {t("layout-editor-settings-title")}
      </h3>
      <p data-part="form.hint">{t("layout-editor-settings-hint")}</p>
      {window === null ? (
        <p data-part="form.hint">{t("layout-editor-settings-no-windows")}</p>
      ) : (
        <>
          <div data-part="form.field" data-field="window">
            <label data-part="form.label" htmlFor={`${id}-window`}>
              {t("layout-editor-settings-window")}
            </label>
            <select
              id={`${id}-window`}
              data-part="form.select"
              value={window}
              onChange={(event) => {
                setChosen(event.target.value);
              }}
            >
              {windows.map((w) => (
                <option key={w} value={w}>
                  {windowName(w)}
                </option>
              ))}
            </select>
          </div>
          {presets.length > 0 && root !== null && (
            <div
              data-part="layout-editor.presets"
              role="group"
              aria-label={t("layout-editor-presets")}
            >
              {presets.map((preset) => {
                const fits = presetFits(preset, root);
                return (
                  <button
                    key={preset.id}
                    type="button"
                    data-part="layout-editor.preset"
                    data-preset={preset.id}
                    aria-pressed={
                      fits &&
                      (pending === null ? presetActive(preset, root) : pending === preset.id)
                    }
                    data-state={pending === preset.id ? "chosen" : undefined}
                    disabled={busy || !fits}
                    onClick={() => {
                      // Choosing the preset the view already has drops the choice.
                      chosenPreset.set(
                        presetActive(preset, root)
                          ? null
                          : { preset: preset.id, window, breakpoint },
                      );
                    }}
                  >
                    <PresetPreview node={preset.root} />
                    <span data-part="layout-editor.preset-name">{presetName(preset)}</span>
                  </button>
                );
              })}
            </div>
          )}
          <div data-part="layout-editor.settings-actions">
            <button
              type="button"
              data-part="form.button"
              data-variant="primary"
              data-action="customize"
              onClick={() => {
                startCustomizing(store, layout, window);
              }}
            >
              <Icon name="layout-dashboard" part="form.button.icon" />
              {t("layout-editor-action-customize")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-action="reset"
              disabled={busy}
              onClick={() => {
                chosenPreset.discard();
                void edit([{ op: "reset-window", window }], {
                  id: "layout-editor-reset-done",
                  args: { window: windowName(window) },
                });
              }}
            >
              <Icon name="rotate-ccw" part="form.button.icon" />
              {t("layout-editor-reset")}
            </button>
          </div>
          <p
            data-part="layout-editor.status"
            data-state={message?.kind === "error" ? "error" : undefined}
            role="status"
          >
            {message ? t(message.text.id, message.text.args) : null}
          </p>
        </>
      )}
    </section>
  );
}
