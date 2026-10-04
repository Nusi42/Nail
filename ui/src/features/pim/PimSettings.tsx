/**
 * The `pim-settings` widget (M5b, WP164): the settings page's "Calendar &
 * contacts" section — the calendars and task lists (settings and remove as
 * buttons), the subscriptions and CalDAV / CardDAV servers with the form to
 * add one, and importing `.ics` / `.vcf` files. The settings search finds
 * it through the labels in `PIM_SETTINGS_INFO` (`Feature.settings`).
 */
import { useId } from "react";
import { useT } from "../../i18n/I18nProvider";
import type { SettingsWidgetInfo } from "../../layout/settingsIndex";
import { useUiStoreApi } from "../../state/uiStore";
import { Icon } from "../../widgets/Icon";
import { CalendarListView } from "./CalendarList";
import { IMPORT_DIALOG } from "./names";
import { SourcesPanel } from "./Sources";

/** Fluent ids of every setting label the section shows (the settings search). */
export const PIM_SETTINGS_INFO: SettingsWidgetInfo = {
  labels: [
    "pim-settings-calendars",
    "pim-calendar-name",
    "pim-calendar-color-label",
    "pim-calendar-reminders",
    "pim-calendar-new-calendar",
    "pim-calendar-new-task-list",
    "pim-settings-sources",
    "pim-source-add",
    "pim-source-feed-url",
    "pim-source-server-url",
    "pim-source-account",
    "pim-source-username",
    "pim-source-refresh",
    "pim-sources-sync-all",
    "pim-settings-import",
  ],
  keywords: "pim-settings-keywords",
};

/** What the calendar list shows wherever a layout places it. */
export const CALENDAR_LIST_INFO: SettingsWidgetInfo = {
  labels: [
    "pim-calendars-label",
    "pim-calendar-name",
    "pim-calendar-color-label",
    "pim-calendar-reminders",
    "pim-calendar-new-calendar",
    "pim-calendar-new-task-list",
    "pim-calendar-new-subscription",
  ],
};

export function PimSettings() {
  const t = useT();
  const ids = { calendars: useId(), sources: useId(), import: useId() };
  const store = useUiStoreApi();
  return (
    <div data-part="pim-settings">
      <section data-part="card" data-field="calendars" aria-labelledby={ids.calendars}>
        <h3 data-part="card.title" id={ids.calendars}>
          {t("pim-settings-calendars")}
        </h3>
        <CalendarListView asButtons bySource landmark={false} />
      </section>
      <section data-part="card" data-field="sources" aria-labelledby={ids.sources}>
        <h3 data-part="card.title" id={ids.sources}>
          {t("pim-settings-sources")}
        </h3>
        <SourcesPanel />
      </section>
      <section data-part="card" data-field="import" aria-labelledby={ids.import}>
        <h3 data-part="card.title" id={ids.import}>
          {t("pim-settings-import")}
        </h3>
        <p data-part="pim.editor.hint">{t("pim-settings-import-hint")}</p>
        <div data-part="pim.editor.actions">
          <button
            type="button"
            data-part="form.button"
            onClick={() => {
              store.getState().openWindow(IMPORT_DIALOG);
            }}
          >
            <Icon name="import" part="form.button.icon" />
            <span data-part="form.button.label">{t("pim-import-open")}</span>
          </button>
        </div>
      </section>
    </div>
  );
}
