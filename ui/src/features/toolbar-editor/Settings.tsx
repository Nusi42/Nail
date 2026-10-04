/**
 * The settings card of the toolbar customizer (widget
 * `toolbar-editor-settings`, M5d WP182): every toolbar of the layout with a
 * button that opens its customizer, and where else it opens from.
 */
import { useId } from "react";
import { useLabel, useT } from "../../i18n/I18nProvider";
import { useLayoutConfig } from "../../layout/LayoutContext";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { useUiStoreApi } from "../../state/uiStore";
import { SETTINGS_PART, customizerWindow } from "./ids";
import { toolbarTitleId } from "./names";

/** Settings → Layout & toolbars → Toolbars. */
export function ToolbarEditorSettings({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const ui = useUiStoreApi();
  const layout = useLayoutConfig();
  const heading = useId();
  const names = Object.keys(layout.toolbars).sort();
  return (
    <section
      data-part={SETTINGS_PART}
      data-node-id={node.id ?? undefined}
      aria-labelledby={heading}
    >
      <h3 id={heading} data-part="toolbar-customizer-settings.title">
        {node.label ? label(node.label) : t("toolbar-editor-settings-title")}
      </h3>
      <p data-part="toolbar-customizer-settings.intro">{t("toolbar-editor-settings-intro")}</p>
      {names.length === 0 ? (
        <p data-part="toolbar-customizer-settings.empty">{t("toolbar-editor-no-toolbars")}</p>
      ) : (
        <ul data-part="toolbar-customizer-settings.list">
          {names.map((name) => {
            const id = toolbarTitleId(name);
            const title = id === null ? name : t(id);
            const count = (layout.toolbars[name] ?? []).filter((i) => i.type === "action").length;
            return (
              <li key={name} data-part="toolbar-customizer-settings.item" data-toolbar={name}>
                <span data-part="toolbar-customizer-settings.name">{title}</span>
                <span data-part="toolbar-customizer-settings.count">
                  {t("toolbar-editor-settings-count", { count })}
                </span>
                <button
                  type="button"
                  data-part="form.button"
                  aria-label={t("toolbar-editor-settings-customize-named", { toolbar: title })}
                  onClick={() => {
                    ui.getState().openWindow(customizerWindow(name));
                  }}
                >
                  {t("toolbar-editor-settings-customize")}
                </button>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
