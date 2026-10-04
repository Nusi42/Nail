/**
 * Pure helpers of printing (ADR 0094, "Printing"): default options, the
 * print view's route, the named CSS page of the options (`@page a4-portrait`
 * in `PRINT_BASE_CSS`), the agenda's default range, the occurrence string of
 * an event instance, and quoted-text folding for "without quoted text".
 */
import type {
  PageOrientation,
  PageSize,
  PimTime,
  PrintOptions,
  PrintTarget,
} from "../../api/generated";
import { ORIENTATIONS, PAGE_SIZES } from "./messages";

/** `nail_api::print::print_view_route`. */
export function printViewRoute(jobId: string): string {
  return `#/print/${jobId}`;
}

/** `nail_api::print::MAX_AGENDA_DAYS`. */
export const MAX_AGENDA_DAYS = 366;

/** `PrintTarget::issues` (the first one), as a Fluent id; `null` = fine. */
export function targetIssue(target: PrintTarget): string | null {
  if (target.type !== "agenda") return null;
  const day = (d: string) =>
    /^\d{4}-\d{2}-\d{2}$/.test(d) && !Number.isNaN(Date.parse(`${d}T00:00:00Z`))
      ? Date.parse(`${d}T00:00:00Z`) / 86_400_000
      : null;
  const start = day(target.start);
  const end = day(target.end);
  if (start === null || end === null) return "print-target-issue-invalid-date";
  if (end < start) return "print-target-issue-end-before-start";
  if (end - start + 1 > MAX_AGENDA_DAYS) return "print-target-issue-range-too-long";
  return null;
}

/** `PrintOptions::default()`. */
export function defaultPrintOptions(): PrintOptions {
  return {
    page_size: "a4",
    orientation: "portrait",
    headers: "basic",
    quoted_text: true,
    attachment_list: true,
    remote_images: false,
    backgrounds: false,
  };
}

/** The job id of a print-view route (`#/print/<id>`), else `null`. */
export function jobIdOfHash(hash: string): string | null {
  const match = /^#\/print\/([^/?#]+)$/.exec(hash);
  if (!match?.[1]) return null;
  try {
    return decodeURIComponent(match[1]);
  } catch {
    return null;
  }
}

/** The CSS named page of a page size and orientation (`a4-portrait`). */
export function pageName(size: PageSize, orientation: PageOrientation): string {
  return `${size}-${orientation}`;
}

/** CSS `size` values of the page sizes. */
const CSS_SIZES: Record<PageSize, string> = {
  a4: "A4",
  a5: "A5",
  letter: "letter",
  legal: "legal",
};

/** `@page` rules for every page size and orientation. */
export function pageRules(): string {
  return PAGE_SIZES.flatMap((size) =>
    ORIENTATIONS.map(
      (o) =>
        `@page ${pageName(size, o)} { size: ${CSS_SIZES[size]} ${o}; margin: 15mm; }\n` +
        `[data-part="print"][data-page="${pageName(size, o)}"] { page: ${pageName(size, o)}; }`,
    ),
  ).join("\n");
}

/** `YYYY-MM-DD` of a date in the local calendar. */
export function isoDay(date: Date): string {
  const y = date.getFullYear().toString().padStart(4, "0");
  const m = (date.getMonth() + 1).toString().padStart(2, "0");
  const d = date.getDate().toString().padStart(2, "0");
  return `${y}-${m}-${d}`;
}

/** The default agenda: this week from today, as a list. */
export function defaultAgenda(now = new Date()): Extract<PrintTarget, { type: "agenda" }> {
  const end = new Date(now);
  end.setDate(end.getDate() + 6);
  return { type: "agenda", start: isoDay(now), end: isoDay(end), calendar_ids: [], layout: "list" };
}

/** The occurrence string of an instance (`RECURRENCE-ID` as listed). */
export function occurrenceOf(recurrence: PimTime | null): string | null {
  if (recurrence === null) return null;
  return recurrence.type === "date" ? recurrence.date : recurrence.local;
}

/** A piece of a message body: text, or a run of quoted lines. */
export interface BodyPiece {
  quoted: boolean;
  text: string;
}

/** Splits a text body into normal and quoted (`>`) runs. */
export function splitQuoted(text: string): BodyPiece[] {
  const pieces: BodyPiece[] = [];
  for (const line of text.split(/\r?\n/)) {
    const quoted = /^\s*>/.test(line);
    const last = pieces[pieces.length - 1];
    if (last?.quoted === quoted) last.text += `\n${line}`;
    else pieces.push({ quoted, text: line });
  }
  return pieces;
}
