/**
 * Small pieces shared by the PIM widgets and dialogs: the write context,
 * the viewer's zone, calendar colours (data, so they go through the theme's
 * geometry rules rather than inline styles), problems shown to the user and
 * the empty state.
 */
import { useQueryClient } from "@tanstack/react-query";
import type { CalendarDate } from "@internationalized/date";
import { useState, type Dispatch, type SetStateAction } from "react";
import type { Calendar, CalendarId, LocalizedMessage } from "../../api/generated";
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
  onChange(value: number): void;
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
      onBlur={() => setText(String(value))}
    />
  );
}
