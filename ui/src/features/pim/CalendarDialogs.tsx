/**
 * The calendar list's dialogs (M5b, WP164), opened from its menus instead
 * of the old inline forms:
 *
 * - `pim-calendar-settings`: name, colour and reminders of a calendar
 *   (read-only calendars keep the changes on this computer);
 * - `pim-new-calendar`: a new calendar or task list in the source of the
 *   group whose "+" menu opened it;
 * - `pim-remove-calendar`: deleting a calendar with its events and tasks,
 *   or unsubscribing from a subscription.
 *
 * Every write goes through the core (`pimWrite`), which queues it.
 */
import { useId, useState, type SubmitEvent } from "react";
import type { Calendar, CalendarSettings, LocalizedMessage } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { isTaskList, removalOf } from "./calendarGroups";
import { CALENDAR_COLORS, isCssColor, paletteColorOf } from "./colors";
import {
  Problem,
  Swatch,
  geomId,
  problemOf,
  useChangeContext,
  useEditorSave,
  SubmitActions,
} from "./common";
import { pimWrite, useCalendars, usePimSources } from "./queries";
import { usePim } from "./store";
import { sourceName } from "./Sources";
import { TextField } from "../../widgets/forms";
import { ConfirmDialog, DialogFooter } from "../../widgets/dialog";

/** Problems of a calendar's name and colour before asking the core. */
export function calendarFormIssues(name: string, color: string): LocalizedMessage | null {
  if (!name.trim()) return { id: "pim-issue-missing-name" };
  if (color.trim() !== "" && !isCssColor(color)) return { id: "pim-calendar-color-invalid" };
  return null;
}

/** The colour choice: the palette as radio swatches plus any other colour. */
export function ColorChoice({
  value,
  onChange,
}: {
  value: string;
  onChange: (color: string) => void;
}) {
  const t = useT();
  const name = useId();
  const customId = useId();
  const current = paletteColorOf(value);
  const [custom, setCustom] = useState(current === null ? value : "");
  return (
    <fieldset data-part="pim.colors" data-field="color">
      <legend data-part="form.label">{t("pim-calendar-color-label")}</legend>
      <div data-part="pim.colors.options">
        {CALENDAR_COLORS.map((color) => (
          <label
            key={color.id}
            data-part="pim.colors.option"
            data-color={color.id}
            title={t(`pim-color-${color.id}`)}
          >
            <input
              type="radio"
              name={name}
              data-part="pim.colors.input"
              value={color.value}
              checked={current?.id === color.id}
              onChange={() => {
                setCustom("");
                onChange(color.value);
              }}
            />
            <Swatch
              geom={geomId("color", name, color.id)}
              color={color.value}
              part="pim.colors.swatch"
            />
            <span data-part="pim.colors.name">{t(`pim-color-${color.id}`)}</span>
          </label>
        ))}
      </div>
      <div data-part="form.field" data-field="custom-color">
        <label data-part="form.label" htmlFor={customId}>
          {t("pim-calendar-color-other")}
        </label>
        <input
          id={customId}
          data-part="form.input"
          value={custom}
          spellCheck={false}
          onChange={(e) => {
            setCustom(e.target.value);
            onChange(e.target.value.trim());
          }}
        />
      </div>
    </fieldset>
  );
}

interface CalendarForm {
  name: string;
  setName: (name: string) => void;
  color: string;
  setColor: (color: string) => void;
  reminders: boolean;
  setReminders: (on: boolean) => void;
}

/** The name, colour and reminders of a calendar dialog, starting at `initial`. */
function useCalendarForm(
  initial: () => {
    name: string;
    color: string;
    reminders: boolean;
  },
): CalendarForm {
  const [first] = useState(initial);
  const [name, setName] = useState(first.name);
  const [color, setColor] = useState(first.color);
  const [reminders, setReminders] = useState(first.reminders);
  return { name, setName, color, setColor, reminders, setReminders };
}

/**
 * A calendar dialog's form and its saving (R6, ADR 0239): the shared editor
 * save (`useEditorSave`), so ×, Esc and Cancel ask with the shared prompt
 * while the name, colour or reminders changed; `write` stores the values.
 */
function useCalendarEditor(
  initial: () => { name: string; color: string; reminders: boolean },
  write: (form: CalendarForm) => Promise<void>,
  close: () => void,
) {
  const form = useCalendarForm(initial);
  const save = useEditorSave({
    // An empty name: the prompt says "Untitled".
    name: form.name.trim(),
    state: { name: form.name, color: form.color, reminders: form.reminders },
    readOnly: false,
    check: () => calendarFormIssues(form.name, form.color),
    write: () => write(form),
    close,
  });
  const onSubmit = (event: SubmitEvent) => {
    event.preventDefault();
    void save.submit();
  };
  return { form, busy: save.busy, problem: save.problem, onSubmit };
}

function CalendarFields({ name, setName, color, setColor, reminders, setReminders }: CalendarForm) {
  const t = useT();
  const ids = { reminders: useId() };
  return (
    <>
      <TextField labelId="pim-calendar-name" name="name" value={name} onChange={setName} />
      <ColorChoice value={color} onChange={setColor} />
      <label data-part="form.check" data-field="reminders">
        <input
          id={ids.reminders}
          type="checkbox"
          checked={reminders}
          onChange={(e) => {
            setReminders(e.target.checked);
          }}
        />
        <span>{t("pim-calendar-reminders")}</span>
      </label>
    </>
  );
}

// ---------------------------------------------------------------------------
// Settings

export function CalendarSettingsDialog({ close }: DialogProps) {
  const t = useT();
  const id = usePim((s) => s.calendarSettingsId);
  const calendars = useCalendars();
  const calendar = calendars.data?.find((c) => c.id === id);
  if (calendars.isPending) return <p data-part="pim.editor.hint">{t("common-loading")}</p>;
  if (!calendar) {
    return (
      <div data-part="pim.calendar-dialog">
        <Problem message={{ id: "pim-calendar-gone" }} />
        <DialogFooter done={{ onDone: close }} />
      </div>
    );
  }
  return <CalendarSettingsForm key={calendar.id} calendar={calendar} close={close} />;
}

function CalendarSettingsForm({ calendar, close }: { calendar: Calendar; close: () => void }) {
  const t = useT();
  const ctx = useChangeContext();
  const { form, busy, problem, onSubmit } = useCalendarEditor(
    () => ({
      name: calendar.settings.name,
      color: calendar.settings.color ?? "",
      reminders: calendar.settings.reminders,
    }),
    async (values) => {
      const settings: CalendarSettings = {
        ...calendar.settings,
        name: values.name.trim(),
        color: values.color.trim() || null,
        reminders: values.reminders,
      };
      await pimWrite(
        ctx,
        { type: "UpdateCalendar", calendar_id: calendar.id, settings },
        "CalendarSaved",
      );
    },
    close,
  );

  return (
    <form
      data-part="pim.calendar-dialog"
      aria-label={t("pim-calendar-settings-of", { name: calendar.settings.name })}
      aria-busy={busy}
      onSubmit={onSubmit}
    >
      {calendar.read_only && <p data-part="pim.editor.hint">{t("pim-calendar-read-only-hint")}</p>}
      <CalendarFields {...form} />
      <SubmitActions
        problem={problem}
        busy={busy}
        submitLabel={t("common-save")}
        onCancel={close}
      />
    </form>
  );
}

// ---------------------------------------------------------------------------
// New calendar / task list

/** The first palette colour no calendar uses yet (else the first). */
export function unusedColor(calendars: readonly Calendar[]): string {
  const used = new Set(calendars.map((c) => (c.settings.color ?? "").toLowerCase()));
  return (CALENDAR_COLORS.find((c) => !used.has(c.value)) ?? CALENDAR_COLORS[0])?.value ?? "";
}

export function NewCalendarDialog({ close }: DialogProps) {
  const t = useT();
  const target = usePim((s) => s.newCalendar);
  const calendars = useCalendars();
  if (calendars.isPending) return <p data-part="pim.editor.hint">{t("common-loading")}</p>;
  return (
    <NewCalendarForm
      sourceId={target?.sourceId ?? "local"}
      component={target?.component ?? "event"}
      calendars={calendars.data ?? []}
      close={close}
    />
  );
}

function NewCalendarForm({
  sourceId,
  component,
  calendars,
  close,
}: {
  sourceId: string;
  component: "event" | "task";
  calendars: readonly Calendar[];
  close: () => void;
}) {
  const t = useT();
  const ctx = useChangeContext();
  const sources = usePimSources();
  const { form, busy, problem, onSubmit } = useCalendarEditor(
    () => ({ name: "", color: unusedColor(calendars), reminders: true }),
    async (values) => {
      await pimWrite(
        ctx,
        {
          type: "CreateCalendar",
          source_id: sourceId,
          settings: {
            name: values.name.trim(),
            description: null,
            color: values.color.trim() || null,
            visible: true,
            reminders: values.reminders,
            default_alarms: [],
            order: calendars.length,
          },
          components: [component],
        },
        "CalendarSaved",
      );
    },
    close,
  );
  const where = sourceName(
    t,
    sources.data?.find((s) => s.id === sourceId),
    sourceId,
  );

  const heading =
    component === "task"
      ? t("pim-calendar-new-task-list-in", { source: where })
      : t("pim-calendar-new-calendar-in", { source: where });
  return (
    <form
      data-part="pim.calendar-dialog"
      data-component={component}
      aria-label={heading}
      aria-busy={busy}
      onSubmit={onSubmit}
    >
      <p data-part="pim.editor.hint">{heading}</p>
      <CalendarFields {...form} />
      <SubmitActions
        problem={problem}
        busy={busy}
        submitLabel={t("common-create")}
        onCancel={close}
      />
    </form>
  );
}

// ---------------------------------------------------------------------------
// Remove

export function RemoveCalendarDialog({ close }: DialogProps) {
  const t = useT();
  const ctx = useChangeContext();
  const id = usePim((s) => s.removeCalendarId);
  const calendars = useCalendars();
  const sources = usePimSources();
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);
  const calendar = calendars.data?.find((c) => c.id === id);
  const removal = calendar ? removalOf(calendar, sources.data ?? []) : null;
  if (calendars.isPending || sources.isPending) {
    return <p data-part="pim.editor.hint">{t("common-loading")}</p>;
  }

  const remove = async () => {
    if (!calendar || removal === null) return;
    setBusy(true);
    setProblem(null);
    try {
      if (removal === "unsubscribe") {
        await pimWrite(ctx, { type: "RemovePimSource", source_id: calendar.source_id }, "Done");
      } else {
        await pimWrite(ctx, { type: "DeleteCalendar", calendar_id: calendar.id }, "PimDeleted");
      }
      close();
    } catch (error) {
      setProblem(problemOf(error));
    } finally {
      setBusy(false);
    }
  };

  const name = calendar?.settings.name ?? "";
  const taskList = calendar !== undefined && isTaskList(calendar);
  const question =
    removal === "unsubscribe"
      ? t("pim-calendar-unsubscribe-question", { name })
      : taskList
        ? t("pim-calendar-remove-task-list-question", { name })
        : t("pim-calendar-remove-question", { name });
  if (!calendar || removal === null) {
    return (
      <div data-part="pim.calendar-dialog">
        <Problem message={{ id: calendar ? "pim-calendar-cannot-remove" : "pim-calendar-gone" }} />
        <DialogFooter done={{ onDone: close }} />
      </div>
    );
  }
  return (
    <ConfirmDialog
      kind={`remove-calendar-${removal}`}
      risk="destructive"
      question={question}
      action={
        removal === "unsubscribe"
          ? t("pim-calendar-unsubscribe")
          : taskList
            ? t("pim-calendar-remove-task-list")
            : t("pim-calendar-remove")
      }
      busy={busy}
      error={problem === null ? null : t(problem.id, problem.args)}
      onConfirm={() => void remove()}
      cancel={{ onClick: close }}
    />
  );
}
