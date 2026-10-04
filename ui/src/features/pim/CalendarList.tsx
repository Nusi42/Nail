/**
 * The `calendar-list` widget (M5b, WP164): calendars and task lists grouped
 * by source (`calendarGroups.ts`). Each row is a colour square — the switch
 * that shows or hides the calendar in the views — the name, and a "…" menu
 * (settings, show / hide, colour, remove or unsubscribe); each group header
 * has a "+" menu (new calendar, new task list, new subscription, the
 * sources dialog, import). Creating, editing and removing happen in the
 * dialogs of `CalendarDialogs.tsx`, not in inline forms.
 *
 * Options: `actions` (`menu`, the default, or `buttons`: settings and
 * remove as icon buttons next to each row) and `show-sources` (a header per
 * source; `false` = one list).
 */
import { useId, useState } from "react";
import type { Calendar, LocalizedMessage } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { optionBool, optionString, type WidgetProps } from "../../layout/widgetRegistry";
import { useGeometry } from "../../theme/ThemeProvider";
import { Icon } from "../../widgets/Icon";
import { calendarGroups, isTaskList, removalOf, type CalendarGroup } from "./calendarGroups";
import { CALENDAR_COLORS, isCssColor, paletteColorOf, swatchDeclarations } from "./colors";
import { Problem, geomId, problemOf, useChangeContext } from "./common";
import {
  CALENDAR_SETTINGS_DIALOG,
  IMPORT_DIALOG,
  NEW_CALENDAR_DIALOG,
  REMOVE_CALENDAR_DIALOG,
  SOURCES_DIALOG,
} from "./names";
import { PimMenu, type PimMenuEntry } from "./PimMenu";
import { pimWrite, useCalendars, usePimSources } from "./queries";
import { sourceName } from "./Sources";

export function CalendarList({ node }: WidgetProps) {
  return (
    <CalendarListView
      asButtons={optionString(node, "actions", "menu") === "buttons"}
      bySource={optionBool(node, "show-sources", true)}
    />
  );
}

/** The list itself (the widget, and the settings page's calendar section). */
export function CalendarListView({
  asButtons,
  bySource,
  landmark = true,
}: {
  asButtons: boolean;
  bySource: boolean;
  /** A `section` landmark (the widget) or a plain group (inside the settings page). */
  landmark?: boolean;
}) {
  const t = useT();
  const ctx = useChangeContext();
  const calendars = useCalendars();
  const sources = usePimSources();
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const sourceList = sources.data ?? [];
  const groups = calendarGroups(calendars.data ?? [], sourceList, bySource);
  const baseId = useId().replace(/[^a-zA-Z0-9_-]/g, "_");

  const save = async (calendar: Calendar, settings: Calendar["settings"]) => {
    setProblem(null);
    try {
      await pimWrite(
        ctx,
        { type: "UpdateCalendar", calendar_id: calendar.id, settings },
        "CalendarSaved",
      );
    } catch (error) {
      setProblem(problemOf(error));
    }
  };
  const open = (name: string) => {
    ctx.store.getState().openWindow(name);
  };
  const pim = () => ctx.pim.getState();

  const groupLabel = (group: CalendarGroup) =>
    group.kind === "subscriptions"
      ? t("pim-calendar-group-subscriptions")
      : group.kind === "all"
        ? t("pim-calendars-label")
        : sourceName(t, group.source ?? undefined, group.source?.id ?? "");

  const newIn = (group: CalendarGroup, component: "event" | "task") => {
    pim().setNewCalendar({ sourceId: group.createIn ?? "local", component });
    open(NEW_CALENDAR_DIALOG);
  };

  const addEntries = (group: CalendarGroup): PimMenuEntry[] => [
    ...(group.createIn !== null
      ? ([
          {
            type: "item",
            id: "new-calendar",
            label: t("pim-calendar-new-calendar"),
            icon: "calendar-plus",
            onSelect: () => {
              newIn(group, "event");
            },
          },
          {
            type: "item",
            id: "new-task-list",
            label: t("pim-calendar-new-task-list"),
            icon: "list-plus",
            onSelect: () => {
              newIn(group, "task");
            },
          },
          { type: "separator", id: "sep-new" },
        ] satisfies PimMenuEntry[])
      : []),
    {
      type: "item",
      id: "new-subscription",
      label: t("pim-calendar-new-subscription"),
      icon: "link",
      onSelect: () => {
        open(SOURCES_DIALOG);
      },
    },
    {
      type: "item",
      id: "sources",
      label: t("pim-sources-open"),
      icon: "server",
      onSelect: () => {
        open(SOURCES_DIALOG);
      },
    },
    {
      type: "item",
      id: "import",
      label: t("pim-import-open"),
      icon: "import",
      onSelect: () => {
        open(IMPORT_DIALOG);
      },
    },
  ];

  const editSettings = (calendar: Calendar) => {
    pim().editCalendar(calendar.id);
    open(CALENDAR_SETTINGS_DIALOG);
  };
  const askRemove = (calendar: Calendar) => {
    pim().setRemoveCalendar(calendar.id);
    open(REMOVE_CALENDAR_DIALOG);
  };

  const rowEntries = (calendar: Calendar): PimMenuEntry[] => {
    const removal = removalOf(calendar, sourceList);
    const current = paletteColorOf(calendar.settings.color);
    const visible = calendar.settings.visible;
    return [
      {
        type: "item",
        id: "settings",
        label: t("pim-calendar-settings-open"),
        icon: "settings",
        onSelect: () => {
          editSettings(calendar);
        },
      },
      {
        type: "item",
        id: "visibility",
        label: visible ? t("pim-calendar-hide") : t("pim-calendar-show"),
        icon: visible ? "eye-off" : "eye",
        onSelect: () => void save(calendar, { ...calendar.settings, visible: !visible }),
      },
      { type: "header", id: "colors", label: t("pim-calendar-color-label") },
      ...CALENDAR_COLORS.map((color): PimMenuEntry => ({
        type: "item",
        id: `color-${color.id}`,
        label: t(`pim-color-${color.id}`),
        color: color.value,
        checked: current?.id === color.id,
        onSelect: () => void save(calendar, { ...calendar.settings, color: color.value }),
      })),
      ...(removal !== null
        ? ([
            { type: "separator", id: "sep-remove" },
            {
              type: "item",
              id: "remove",
              label:
                removal === "unsubscribe"
                  ? t("pim-calendar-unsubscribe-menu")
                  : t("pim-calendar-remove-menu"),
              icon: "trash",
              danger: true,
              onSelect: () => {
                askRemove(calendar);
              },
            },
          ] satisfies PimMenuEntry[])
        : []),
    ];
  };

  const Root = landmark ? "section" : "div";
  return (
    <Root
      data-part="calendar-list"
      data-actions={asButtons ? "buttons" : "menu"}
      role={landmark ? undefined : "group"}
      aria-label={t("pim-calendars-label")}
    >
      {groups.map((group) => {
        const label = groupLabel(group);
        const headingId = `${baseId}-${group.key}`;
        return (
          <div
            key={group.key}
            role="group"
            data-part="calendar-list.source"
            data-kind={group.kind}
            data-source={group.source?.id}
            aria-labelledby={headingId}
          >
            <div data-part="calendar-list.source.header">
              <h3 data-part="calendar-list.source.name" id={headingId}>
                {label}
              </h3>
              <PimMenu
                label={t("pim-calendar-add-to", { group: label })}
                icon="plus"
                field="calendar-add"
                part="calendar-list.add"
                entries={addEntries(group)}
              />
            </div>
            {group.calendars.length > 0 && (
              <ul data-part="calendar-list.items">
                {group.calendars.map((calendar) => (
                  <CalendarRow
                    key={calendar.id}
                    calendar={calendar}
                    asButtons={asButtons}
                    entries={rowEntries(calendar)}
                    removable={removalOf(calendar, sourceList) !== null}
                    onToggle={(visible) => void save(calendar, { ...calendar.settings, visible })}
                    onSettings={() => {
                      editSettings(calendar);
                    }}
                    onRemove={() => {
                      askRemove(calendar);
                    }}
                  />
                ))}
              </ul>
            )}
          </div>
        );
      })}
      <Problem message={problem} />
    </Root>
  );
}

interface RowProps {
  calendar: Calendar;
  asButtons: boolean;
  entries: readonly PimMenuEntry[];
  removable: boolean;
  onToggle: (visible: boolean) => void;
  onSettings: () => void;
  onRemove: () => void;
}

function CalendarRow({
  calendar,
  asButtons,
  entries,
  removable,
  onToggle,
  onSettings,
  onRemove,
}: RowProps) {
  const t = useT();
  const geom = geomId("toggle", calendar.id);
  useGeometry(geom, swatchDeclarations(calendar.settings.color));
  const name = calendar.settings.name;
  const states = [
    calendar.settings.visible ? null : "hidden",
    calendar.read_only ? "read-only" : null,
  ].filter(Boolean);
  return (
    <li
      data-part="calendar-list.item"
      data-calendar={calendar.id}
      data-state={states.join(" ") || undefined}
    >
      <label data-part="calendar-list.label">
        <input
          type="checkbox"
          data-part="calendar-list.toggle"
          data-geom={geom}
          data-colored={isCssColor(calendar.settings.color) ? "" : undefined}
          checked={calendar.settings.visible}
          onChange={(e) => {
            onToggle(e.target.checked);
          }}
        />
        <span data-part="calendar-list.name">{name}</span>
        {isTaskList(calendar) && (
          <span data-part="calendar-list.kind">{t("pim-calendar-kind-tasks")}</span>
        )}
      </label>
      {asButtons ? (
        <span data-part="calendar-list.buttons">
          <button
            type="button"
            data-part="form.button"
            data-variant="icon"
            aria-label={t("pim-calendar-settings-of", { name })}
            title={t("pim-calendar-settings-of", { name })}
            onClick={onSettings}
          >
            <Icon name="settings" part="form.button.icon" />
          </button>
          {removable && (
            <button
              type="button"
              data-part="form.button"
              data-variant="icon"
              aria-label={t("pim-calendar-remove-of", { name })}
              title={t("pim-calendar-remove-of", { name })}
              onClick={onRemove}
            >
              <Icon name="trash" part="form.button.icon" />
            </button>
          )}
        </span>
      ) : (
        <PimMenu
          label={t("pim-calendar-actions-of", { name })}
          icon="ellipsis"
          field="calendar-actions"
          part="calendar-list.menu"
          entries={entries}
        />
      )}
    </li>
  );
}
