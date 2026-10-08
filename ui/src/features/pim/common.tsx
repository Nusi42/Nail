/**
 * Small pieces shared by the PIM widgets and dialogs: the write context,
 * the viewer's zone, calendar colours (data, so they go through the theme's
 * geometry rules rather than inline styles), problems shown to the user and
 * the empty state.
 */
import { useQueryClient } from "@tanstack/react-query";
import type { CalendarDate } from "@internationalized/date";
import { useState, type Dispatch, type ReactNode, type SetStateAction } from "react";
import type { AddressBook, Calendar, CalendarId, LocalizedMessage } from "../../api/generated";
import { errorMessageOf } from "../../api/client";
import { useT } from "../../i18n/I18nProvider";
import { optionString, type WidgetNode } from "../../layout/widgetRegistry";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { useGeometry } from "../../theme/ThemeProvider";
import type { ChangeContext } from "./changes";
import { eventCardDeclarations, swatchDeclarations } from "./colors";
import { usePimStore } from "./store";
import { isKnownZone, localZone } from "./time";
import { TextField } from "../../widgets/forms";

/** Everything a PIM write from a component needs. */
export function useChangeContext(): ChangeContext {
  return {
    client: useClient(),
    queryClient: useQueryClient(),
    store: useUiStoreApi(),
    pim: usePimStore(),
  };
}

/**
 * A day that follows `value` (the shared anchor) but can move on its own
 * (keyboard focus in a grid); it jumps back whenever `value` changes.
 */
export function useFollowingDay(
  value: CalendarDate,
): [CalendarDate, Dispatch<SetStateAction<CalendarDate>>] {
  const [day, setDay] = useState(value);
  const [last, setLast] = useState(value);
  if (last.compare(value) !== 0) {
    setLast(value);
    setDay(value);
  }
  return [day, setDay];
}

/** The viewer's zone: the widget's `time-zone` option, else the system's. */
export function useViewZone(node?: WidgetNode): string {
  const option = node ? optionString(node, "time-zone", "") : "";
  return option && isKnownZone(option) ? option : localZone();
}

/** The user-facing text of a failed request. */
export function problemOf(error: unknown): LocalizedMessage {
  return errorMessageOf(error);
}

/**
 * The main fields of an editor dialog: disabled while saving or for a
 * read-only item, which says so above them.
 */
export function EditorMain({
  readOnly,
  busy,
  children,
}: {
  readOnly: boolean;
  busy: boolean;
  children: ReactNode;
}) {
  const t = useT();
  return (
    <>
      {readOnly && <p data-part="pim.editor.read-only">{t("pim-editor-read-only")}</p>}
      <fieldset data-part="pim.editor.main" disabled={readOnly || busy}>
        {children}
      </fieldset>
    </>
  );
}

/** A dialog's problem and its actions: Cancel, then the submit button. */
export function SubmitActions({
  problem,
  busy,
  submitLabel,
  onCancel,
}: {
  problem: LocalizedMessage | null;
  busy: boolean;
  submitLabel: string;
  onCancel: () => void;
}) {
  const t = useT();
  return (
    <>
      <Problem message={problem} />
      <div data-part="pim.editor.actions">
        <button type="button" data-part="form.button" onClick={onCancel}>
          {t("common-cancel")}
        </button>
        <button type="submit" data-part="form.button" data-variant="primary" disabled={busy}>
          {submitLabel}
        </button>
      </div>
    </>
  );
}

/** The calendars (or task lists) of a select, by name. */
export function CalendarOptions({ calendars }: { calendars: readonly Calendar[] }) {
  return (
    <>
      {calendars.map((c) => (
        <option key={c.id} value={c.id}>
          {c.settings.name}
        </option>
      ))}
    </>
  );
}

/** The address books of a select, by name (the collected addresses by their role). */
export function BookOptions({ books }: { books: readonly AddressBook[] }) {
  const t = useT();
  return (
    <>
      {books.map((b) => (
        <option key={b.id} value={b.id}>
          {b.role === "collected" ? t("pim-book-collected") : b.settings.name}
        </option>
      ))}
    </>
  );
}

/** The categories of an item as one comma-separated field. */
export function CategoriesField({
  id,
  categories,
  onChange,
}: {
  id: string;
  categories: readonly string[];
  onChange: (categories: string[]) => void;
}) {
  return (
    <TextField
      id={id}
      name="categories"
      labelId="pim-categories"
      value={categories.join(", ")}
      onChange={(v) => {
        onChange(v.split(","));
      }}
    />
  );
}

/**
 * A section of an editor whose value may be unset (a contact's birthday, a
 * task's start or due date): the check box `toggleText` sets or clears it,
 * `children` edit it while it is set.
 */
export function OptionalSection({
  field,
  label,
  toggleText,
  set,
  onToggle,
  children,
}: {
  field: string;
  label: string;
  toggleText: string;
  set: boolean;
  onToggle: (on: boolean) => void;
  children: ReactNode;
}) {
  return (
    <fieldset data-part="pim.editor.section" data-section={field}>
      <legend data-part="form.section.title">{label}</legend>
      <label data-part="form.check" data-field={`${field}-set`}>
        <input
          type="checkbox"
          checked={set}
          onChange={(e) => {
            onToggle(e.target.checked);
          }}
        />
        <span>{toggleText}</span>
      </label>
      {children}
    </fieldset>
  );
}

/** A problem shown under a form or list (`role="alert"`). */
export function Problem({
  message,
  part = "pim.problem",
  id,
}: {
  message: LocalizedMessage | null;
  part?: string;
  id?: string;
}) {
  const t = useT();
  if (message === null) return null;
  return (
    <p data-part={part} role="alert" id={id}>
      {t(message.id, message.args)}
    </p>
  );
}

/** The colour of an event: its own, else its calendar's (`null` = the theme's). */
export function colorOf(
  calendars: readonly Calendar[] | undefined,
  calendarId: CalendarId,
  own: string | null,
): string | null {
  return own ?? calendars?.find((c) => c.id === calendarId)?.settings.color ?? null;
}

/**
 * Declarations for an event card showing a data colour: a bar in the colour
 * and a light tint of it (`colors.ts`); the theme decides everything else.
 */
export function colorDeclarations(color: string | null): Record<string, string> {
  return eventCardDeclarations(color);
}

/** A calendar's colour swatch (`pim.swatch`, coloured through a geometry rule). */
export function Swatch({
  geom,
  color,
  part = "pim.swatch",
}: {
  geom: string;
  color: string | null;
  part?: string;
}) {
  useGeometry(geom, swatchDeclarations(color));
  return <span data-part={part} data-geom={geom} aria-hidden="true" />;
}

/** A stable geometry id from parts. */
export function geomId(...parts: (string | number | null | undefined)[]): string {
  return `pim-${parts
    .map((p) => String(p ?? ""))
    .join("-")
    .replace(/[^a-zA-Z0-9_-]/g, "_")}`;
}

/**
 * A whole-number field that can be emptied while typing: it reports valid
 * numbers (clamped to `min`–`max`) and shows the last valid one again when
 * it loses focus.
 */
export function NumberField({
  id,
  value,
  min,
  max,
  onChange,
}: {
  id: string;
  value: number;
  min: number;
  max: number;
  onChange: (value: number) => void;
}) {
  const [text, setText] = useState(String(value));
  const [last, setLast] = useState(value);
  if (last !== value) {
    setLast(value);
    if (Number(text) !== value) setText(String(value));
  }
  return (
    <input
      id={id}
      data-part="form.input"
      data-width="short"
      type="number"
      inputMode="numeric"
      min={min}
      max={max}
      value={text}
      onChange={(event) => {
        setText(event.target.value);
        const parsed = Math.trunc(Number(event.target.value));
        if (event.target.value.trim() !== "" && Number.isFinite(parsed)) {
          onChange(Math.min(max, Math.max(min, parsed)));
        }
      }}
      onBlur={() => {
        setText(String(value));
      }}
    />
  );
}
