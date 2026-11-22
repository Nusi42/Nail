/**
 * Small pieces shared by the PIM widgets and dialogs: the write context,
 * the viewer's zone, calendar colours (data, so they go through the theme's
 * geometry rules rather than inline styles), problems shown to the user and
 * the empty state.
 */
import { useQueryClient } from "@tanstack/react-query";
import type { CalendarDate } from "@internationalized/date";
import { useId, useRef, useState, type Dispatch, type ReactNode, type SetStateAction } from "react";
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
import { DialogFooter, useCloseGuard } from "../../widgets/dialog";

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

/**
 * A form dialog's problem and its footer (R6, ADR 0239): the verb (the
 * form's submit button) and Cancel by role, in the viewer's system's order.
 */
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
  return (
    <>
      <Problem message={problem} />
      <DialogFooter
        primary={{ label: submitLabel, submit: true, action: "submit" }}
        cancel={{ onClick: onCancel }}
        busy={busy}
      />
    </>
  );
}

/**
 * Saving an editor (R6, ADR 0239): `check` says why the item cannot be
 * saved (shown as the editor's problem), `write` writes it. `persist`
 * saves and reports whether it did; `submit` saves and closes. The close
 * guard asks when ×, Esc or Cancel would drop changes — what differs from
 * `state` as the editor opened — and saves through `persist`. `finish`
 * after the editor deleted its item, so its own close does not ask.
 */
export function useEditorSave({
  name,
  state,
  readOnly,
  canSave = true,
  check,
  write,
  close,
}: {
  /** What is edited, localized (its title). */
  name: string;
  /** The editor's state (compared as JSON). */
  state: unknown;
  readOnly: boolean;
  /** More conditions for saving than `check` (a chosen calendar). */
  canSave?: boolean;
  /** Why the item cannot be saved, `null` when it can. */
  check: () => LocalizedMessage | null;
  /** Writes the item. */
  write: () => Promise<void>;
  close: () => void;
}) {
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);
  const [pristine] = useState(() => JSON.stringify(state));
  const finished = useRef(false);
  const dirty = !readOnly && JSON.stringify(state) !== pristine;
  const finish = () => {
    finished.current = true;
  };
  /** Saves: `true` when saved (the editor may close). */
  const persist = async (): Promise<boolean> => {
    const issue = check();
    if (issue) {
      setProblem(issue);
      return false;
    }
    setBusy(true);
    setProblem(null);
    try {
      await write();
      finish();
      return true;
    } catch (error) {
      setProblem(problemOf(error));
      return false;
    } finally {
      setBusy(false);
    }
  };
  useCloseGuard({
    name: () => name,
    isDirty: () => !finished.current && dirty,
    canSave: () => canSave && check() === null,
    save: persist,
  });
  return {
    problem,
    setProblem,
    busy,
    dirty,
    persist,
    finish,
    /** Saves, then closes the editor. */
    submit: async () => {
      if (await persist()) close();
    },
  };
}

/**
 * An editor's footer: `Delete…` alone at the start (it asks first; left out
 * for a new or read-only item), Save (the form's submit button) and Cancel by
 * role.
 */
export function EditorFooter({
  onDelete,
  readOnly,
  saveDisabled,
  busy,
  close,
}: {
  onDelete: (() => void) | null;
  readOnly: boolean;
  saveDisabled: boolean;
  busy: boolean;
  close: () => void;
}) {
  const t = useT();
  return (
    <DialogFooter
      {...(onDelete && !readOnly
        ? {
            start: {
              label: t("pim-delete-ask"),
              destructive: true,
              action: "delete",
              onClick: onDelete,
            },
          }
        : {})}
      {...(readOnly
        ? {}
        : {
            primary: {
              label: t("common-save"),
              submit: true,
              action: "save",
              disabled: saveDisabled,
            },
          })}
      cancel={{ onClick: close }}
      busy={busy}
    />
  );
}

/** Radio buttons for one of `values` (a scope), each labelled `label(value)`. */
export function ScopeRadios<V extends string>({
  part,
  legend,
  values,
  value,
  label,
  onChange,
  children,
}: {
  part: string;
  legend: string;
  values: readonly V[];
  value: V;
  label: (value: V) => string;
  onChange: (value: V) => void;
  /** More fields under the choices (a day). */
  children?: ReactNode;
}) {
  const name = useId();
  return (
    <fieldset data-part={part}>
      <legend data-part="form.section.title">{legend}</legend>
      {values.map((v) => (
        <label key={v} data-part="form.check" data-scope={v}>
          <input
            type="radio"
            name={name}
            value={v}
            checked={value === v}
            onChange={() => {
              onChange(v);
            }}
          />
          <span>{label(v)}</span>
        </label>
      ))}
      {children}
    </fieldset>
  );
}

/** An editor whose item or lists failed to load: the reason and Done. */
export function EditorLoadError({ error, close }: { error: unknown; close: () => void }) {
  return (
    <div data-part="pim.editor" data-state="error">
      <Problem message={problemOf(error)} />
      <DialogFooter done={{ onDone: close }} />
    </div>
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
