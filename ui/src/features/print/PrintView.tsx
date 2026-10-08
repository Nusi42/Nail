/**
 * The print view (route `#/print/<job id>`, ADR 0094) and the effect that
 * shows it (`PrintHost`).
 *
 * The view reads the print job (`GetPrintJob`), renders the message,
 * conversation, event or agenda with the theme's `print.*` parts — on top
 * of the app, which the print style sheet hides — and, once everything is
 * loaded, reports `PrintViewReady` (the desktop shell waits for it before
 * writing a PDF in a hidden window). While it is shown the document's
 * colour mode is `print`: a theme's `print` mode tokens (`print=` values)
 * apply, and without them the light base values, so dark mode never
 * prints white on black. If this window asked to print, the system print
 * dialog opens (`window.print()`; in a browser it also saves PDFs); when
 * it closes, the view closes too.
 *
 * The host also prints local sheets without a job (the recovery kit).
 */
import { useIsFetching, useQuery } from "@tanstack/react-query";
import { useEffect, useLayoutEffect, useState, type ReactNode } from "react";
import { createPortal } from "react-dom";
import { errorMessageOf } from "../../api/client";
import type {
  CalendarEvent,
  EventInstance,
  MessageBody,
  PrintJob,
  PrintOptions,
  PrintTarget,
} from "../../api/generated";
import { useLocale, useT } from "../../i18n/I18nProvider";
import { useDateFormat } from "../../i18n/format";
import { useClient, useMessage, useThread } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { formatClock, formatDay, formatPimTime } from "../pim/fmt";
import { daysOf, localZone, monthGrid, overlapsDay, parseDay, rangeInstants } from "../pim/time";
import { formatBytes } from "../portability/model";
import { jobIdOfHash, pageName, splitQuoted } from "./model";
import { printStoreFor, usePrintState } from "./store";
import { CloseButton } from "../../widgets/CloseButton";

/** Query key of a print job. */
export const printJobKey = (jobId: string) => ["print", "job", jobId] as const;

function useHash(): string {
  const [hash, setHash] = useState(() => window.location.hash);
  useEffect(() => {
    const update = () => {
      setHash(window.location.hash);
    };
    window.addEventListener("hashchange", update);
    return () => {
      window.removeEventListener("hashchange", update);
    };
  }, []);
  return hash;
}

/** Leaves the print view (back to the app). */
export function closePrintView(): void {
  if (jobIdOfHash(window.location.hash) === null) return;
  // Replace, so Back does not reopen the print view.
  window.history.replaceState(null, "", `${window.location.pathname}${window.location.search}`);
  window.dispatchEvent(new HashChangeEvent("hashchange"));
}

/** Opens the print view of a job. */
export function openPrintView(view: string): void {
  window.location.hash = view.startsWith("#") ? view.slice(1) : view;
}

/** The document's colour mode is `print` while the view is shown. */
function usePrintMode(): void {
  useLayoutEffect(() => {
    const root = document.documentElement;
    const previous = root.dataset.nailMode;
    root.dataset.nailMode = "print";
    return () => {
      if (previous === undefined) delete root.dataset.nailMode;
      else root.dataset.nailMode = previous;
    };
  }, []);
}

function Addresses({
  header,
  list,
}: {
  header: "from" | "to" | "cc" | "reply-to";
  list: readonly { name: string | null; email: string }[];
}) {
  const t = useT();
  if (list.length === 0) return null;
  return (
    <div data-part="print.header" data-header={header}>
      <dt data-part="print.header.name">{t(`print-header-${header}`)}</dt>
      <dd data-part="print.header.value">
        {list
          .map((a) => (a.name ? t("common-name-email", { name: a.name, email: a.email }) : a.email))
          .join(", ")}
      </dd>
    </div>
  );
}

function Body({ body, options }: { body: MessageBody; options: PrintOptions }) {
  const t = useT();
  const pieces = splitQuoted(body.text);
  return (
    <div data-part="print.body" data-backgrounds={options.backgrounds ? "" : undefined}>
      {pieces.map((piece, i) =>
        piece.quoted ? (
          options.quoted_text ? (
            <blockquote key={i} data-part="print.quote">
              {piece.text}
            </blockquote>
          ) : (
            <p key={i} data-part="print.quote-omitted">
              {t("print-quote-omitted")}
            </p>
          )
        ) : (
          <div key={i} data-part="print.text">
            {piece.text}
          </div>
        ),
      )}
    </div>
  );
}

/** One message: headers, text, attachments. */
export function MessagePrint({ body, options }: { body: MessageBody; options: PrintOptions }) {
  const t = useT();
  const locale = useLocale();
  const format = useDateFormat();
  const h = body.headers;
  // The attachments the reader lists: not the images the body shows (ADR 0162).
  const attachments = body.attachments.filter((a) => !a.shown_in_body);
  return (
    <article data-part="print.message">
      <h1 data-part="print.subject">{h.subject || t("common-no-subject")}</h1>
      <dl data-part="print.headers" data-detail={options.headers}>
        <Addresses header="from" list={h.from} />
        <Addresses header="to" list={h.to} />
        <Addresses header="cc" list={h.cc} />
        {options.headers === "full" && <Addresses header="reply-to" list={h.reply_to} />}
        {h.date && (
          <div data-part="print.header" data-header="date">
            <dt data-part="print.header.name">{t("common-date")}</dt>
            <dd data-part="print.header.value">{format.dateTime(h.date)}</dd>
          </div>
        )}
        {options.headers === "full" && h.message_id && (
          <div data-part="print.header" data-header="message-id">
            <dt data-part="print.header.name">{t("print-header-message-id")}</dt>
            <dd data-part="print.header.value">{h.message_id}</dd>
          </div>
        )}
      </dl>
      <Body body={body} options={options} />
      {options.attachment_list && attachments.length > 0 && (
        <section data-part="print.attachments" aria-label={t("common-attachments")}>
          <h2 data-part="print.section.title">{t("common-attachments")}</h2>
          <ul data-part="print.attachments.list">
            {attachments.map((a) => (
              <li key={a.index} data-part="print.attachment">
                {t("print-attachment", {
                  name: a.filename ?? t("common-unnamed-attachment"),
                  size: formatBytes(a.size, locale),
                })}
              </li>
            ))}
          </ul>
        </section>
      )}
    </article>
  );
}

function LoadedMessage({ id, options }: { id: string; options: PrintOptions }) {
  const message = useMessage(id);
  if (message.isError) return <Problem error={message.error} />;
  if (!message.data) return null;
  return <MessagePrint body={message.data} options={options} />;
}

function ThreadPrint({ threadId, options }: { threadId: string; options: PrintOptions }) {
  const t = useT();
  const thread = useThread(threadId);
  if (thread.isError) return <Problem error={thread.error} />;
  if (!thread.data) return null;
  const messages = [...thread.data.messages].sort((a, b) => a.date.localeCompare(b.date));
  return (
    <section data-part="print.thread">
      <h1 data-part="print.thread.title">{thread.data.thread.subject || t("common-no-subject")}</h1>
      <p data-part="print.thread.count">{t("common-message-count", { count: messages.length })}</p>
      {messages.map((m) => (
        <LoadedMessage key={m.id} id={m.id} options={options} />
      ))}
    </section>
  );
}

function EventPrint({ eventId, occurrence }: { eventId: string; occurrence: string | null }) {
  const t = useT();
  const format = useDateFormat();
  const client = useClient();
  const tz = localZone();
  const event = useQuery({
    queryKey: ["print", "event", eventId],
    queryFn: async (): Promise<CalendarEvent> =>
      (await client.request({ type: "GetEvent", event_id: eventId }, "PimEvent")).event,
  });
  if (event.isError) return <Problem error={event.error} />;
  if (!event.data) return null;
  const f = event.data.fields;
  return (
    <article data-part="print.event">
      <h1 data-part="print.event.title">{f.title || t("print-no-title")}</h1>
      <dl data-part="print.headers">
        <div data-part="print.header" data-header="when">
          <dt data-part="print.header.name">{t("common-when")}</dt>
          <dd data-part="print.header.value">
            {t("common-time-range", {
              start: formatPimTime(format, f.start, tz),
              end: formatPimTime(format, f.end, tz),
            })}
          </dd>
        </div>
        {occurrence && (
          <div data-part="print.header" data-header="occurrence">
            <dt data-part="print.header.name">{t("common-occurrence")}</dt>
            <dd data-part="print.header.value">{occurrence}</dd>
          </div>
        )}
        {f.location && (
          <div data-part="print.header" data-header="location">
            <dt data-part="print.header.name">{t("common-where")}</dt>
            <dd data-part="print.header.value">{f.location}</dd>
          </div>
        )}
        {f.organizer && (
          <div data-part="print.header" data-header="organizer">
            <dt data-part="print.header.name">{t("common-organizer")}</dt>
            <dd data-part="print.header.value">{f.organizer.name ?? f.organizer.email}</dd>
          </div>
        )}
      </dl>
      {f.description && <div data-part="print.text">{f.description}</div>}
      {f.attendees.length > 0 && (
        <section data-part="print.attendees" aria-label={t("common-attendees")}>
          <h2 data-part="print.section.title">{t("common-attendees")}</h2>
          <ul data-part="print.attendees.list">
            {f.attendees.map((a) => (
              <li key={a.email} data-part="print.attendee" data-status={a.status}>
                {t("print-attendee", {
                  name: a.name ?? a.email,
                  status: t(`pim-partstat-${a.status}`),
                })}
              </li>
            ))}
          </ul>
        </section>
      )}
    </article>
  );
}

function InstanceLine({ instance, tz }: { instance: EventInstance; tz: string }) {
  const t = useT();
  const format = useDateFormat();
  return (
    <li data-part="print.agenda.event" data-all-day={instance.all_day ? "" : undefined}>
      <span data-part="print.agenda.time">
        {instance.all_day
          ? t("common-all-day")
          : t("common-time-range-tight", {
              start: formatClock(format, instance.start_utc, tz),
              end: formatClock(format, instance.end_utc, tz),
            })}
      </span>
      <span data-part="print.agenda.title">{instance.title || t("print-no-title")}</span>
      {instance.location && <span data-part="print.agenda.location">{instance.location}</span>}
    </li>
  );
}

function AgendaPrint({ target }: { target: Extract<PrintTarget, { type: "agenda" }> }) {
  const t = useT();
  const format = useDateFormat();
  const locale = useLocale();
  const client = useClient();
  const tz = localZone();
  const start = parseDay(target.start);
  const end = parseDay(target.end);
  const range = start && end ? { start, count: end.compare(start) + 1 } : null;
  const instants = range ? rangeInstants(range, tz) : null;
  const instances = useQuery({
    queryKey: ["print", "agenda", target.start, target.end, target.calendar_ids, tz],
    enabled: instants !== null,
    queryFn: async (): Promise<EventInstance[]> =>
      (
        await client.request(
          {
            type: "ListEventInstances",
            from: instants?.from ?? "",
            to: instants?.to ?? "",
            calendar_ids: target.calendar_ids.length > 0 ? target.calendar_ids : null,
            tz,
          },
          "EventInstances",
        )
      ).instances,
  });
  if (!range) return null;
  if (instances.isError) return <Problem error={instances.error} />;
  if (!instances.data) return null;
  const list = [...instances.data].sort((a, b) => a.start_utc.localeCompare(b.start_utc));
  const dayEvents = (day: ReturnType<typeof daysOf>[number]) =>
    list.filter((i) => overlapsDay(i, day, tz));
  const title = t("common-time-range", {
    start: formatDay(format, range.start, { dateStyle: "long" }),
    end: formatDay(format, range.start.add({ days: range.count - 1 }), { dateStyle: "long" }),
  });

  if (target.layout === "month") {
    const months: { first: typeof range.start; weeks: ReturnType<typeof monthGrid> }[] = [];
    let cursor = range.start.set({ day: 1 });
    while (cursor.compare(range.start.add({ days: range.count - 1 })) <= 0) {
      months.push({ first: cursor, weeks: monthGrid(cursor, locale) });
      cursor = cursor.add({ months: 1 });
    }
    return (
      <section data-part="print.agenda" data-layout="month">
        <h1 data-part="print.agenda.heading">{title}</h1>
        {months.map(({ first, weeks }) => (
          <table key={first.toString()} data-part="print.agenda.month">
            <caption data-part="print.agenda.caption">
              {formatDay(format, first, { month: "long", year: "numeric" })}
            </caption>
            <tbody>
              {weeks.map((week, w) => (
                <tr key={w} data-part="print.agenda.week-row">
                  {week.map((day) => (
                    <td key={day.toString()} data-part="print.agenda.cell">
                      <span data-part="print.agenda.cell.day">
                        {formatDay(format, day, { day: "numeric", weekday: "short" })}
                      </span>
                      <ul data-part="print.agenda.events">
                        {dayEvents(day).map((i) => (
                          <InstanceLine key={`${i.event_id}-${i.start_utc}`} instance={i} tz={tz} />
                        ))}
                      </ul>
                    </td>
                  ))}
                </tr>
              ))}
            </tbody>
          </table>
        ))}
      </section>
    );
  }

  const days = daysOf(range);
  const groups: (typeof days)[] =
    target.layout === "week"
      ? Array.from({ length: Math.ceil(days.length / 7) }, (_, i) => days.slice(i * 7, i * 7 + 7))
      : [days];
  return (
    <section data-part="print.agenda" data-layout={target.layout}>
      <h1 data-part="print.agenda.heading">{title}</h1>
      {groups.map((group, g) => (
        <div key={g} data-part="print.agenda.group">
          {group.map((day) => {
            const events = dayEvents(day);
            if (target.layout === "list" && events.length === 0) return null;
            return (
              <section key={day.toString()} data-part="print.agenda.day">
                <h2 data-part="print.agenda.date">
                  {formatDay(format, day, { dateStyle: "full" })}
                </h2>
                {events.length === 0 ? (
                  <p data-part="print.agenda.free">{t("print-agenda-free")}</p>
                ) : (
                  <ul data-part="print.agenda.events">
                    {events.map((i) => (
                      <InstanceLine key={`${i.event_id}-${i.start_utc}`} instance={i} tz={tz} />
                    ))}
                  </ul>
                )}
              </section>
            );
          })}
        </div>
      ))}
      {list.length === 0 && <p data-part="print.agenda.free">{t("print-agenda-empty")}</p>}
    </section>
  );
}

function Problem({ error }: { error: unknown }) {
  const t = useT();
  const problem = errorMessageOf(error);
  return (
    <p data-part="print.error" role="alert">
      {t(problem.id, problem.args)}
    </p>
  );
}

function TargetPrint({ job }: { job: PrintJob }) {
  const target = job.target;
  switch (target.type) {
    case "message":
      return <LoadedMessage id={target.message_id} options={job.options} />;
    case "thread":
      return <ThreadPrint threadId={target.thread_id} options={job.options} />;
    case "event":
      return <EventPrint eventId={target.event_id} occurrence={target.occurrence} />;
    case "agenda":
      return <AgendaPrint target={target} />;
  }
}

/** The screen-only bar above the page (hidden when printing). */
function Toolbar({ onPrint, onClose }: { onPrint: () => void; onClose: () => void }) {
  const t = useT();
  return (
    <div data-part="print.toolbar" role="toolbar" aria-label={t("print-toolbar")}>
      <button
        type="button"
        data-part="form.button"
        data-variant="primary"
        data-action="print"
        onClick={onPrint}
      >
        {t("common-print")}
      </button>
      <CloseButton onClose={onClose} action="close-print" />
    </div>
  );
}

/** Calls `window.print()` and reports when the dialog closed. */
function printNow(after: () => void): void {
  let done = false;
  const finish = () => {
    if (done) return;
    done = true;
    window.removeEventListener("afterprint", finish);
    after();
  };
  window.addEventListener("afterprint", finish);
  try {
    window.print();
  } finally {
    // Some hosts never send `afterprint`; `print()` blocks until closed there.
    setTimeout(finish, 0);
  }
}

/** The print view of one job. */
export function PrintView({ jobId }: { jobId: string }) {
  const t = useT();
  const client = useClient();
  const ui = useUiStoreApi();
  const fetching = useIsFetching();
  const [reported, setReported] = useState(false);
  usePrintMode();
  const job = useQuery({
    queryKey: printJobKey(jobId),
    retry: false,
    queryFn: async () =>
      (await client.request({ type: "GetPrintJob", job_id: jobId }, "PrintJob")).job,
  });

  // Laid out once the job and everything it shows are loaded.
  useEffect(() => {
    if (!job.data || fetching > 0 || reported) return;
    const frame = requestAnimationFrame(() => {
      setReported(true);
      void client.call({ type: "PrintViewReady", job_id: jobId });
      const store = printStoreFor(ui);
      if (store.getState().autoPrint === jobId) {
        store.getState().setAutoPrint(null);
        printNow(closePrintView);
      }
    });
    return () => {
      cancelAnimationFrame(frame);
    };
  }, [job.data, fetching, reported, client, jobId, ui]);

  const options = job.data?.options;
  return (
    <div
      data-part="print"
      data-target={job.data?.target.type}
      data-page={options ? pageName(options.page_size, options.orientation) : undefined}
      data-state={reported ? "ready" : "loading"}
      role="document"
      aria-label={t("print-toolbar")}
    >
      <Toolbar
        onPrint={() => {
          printNow(() => undefined);
        }}
        onClose={closePrintView}
      />
      <div data-part="print.page">
        {job.isError ? (
          <Problem error={job.error} />
        ) : job.data ? (
          <TargetPrint job={job.data} />
        ) : null}
      </div>
    </div>
  );
}

/** A local sheet printed without a job. */
function SheetView({ sheet }: { sheet: ReactNode }) {
  const ui = useUiStoreApi();
  usePrintMode();
  useEffect(() => {
    const frame = requestAnimationFrame(() => {
      printNow(() => {
        printStoreFor(ui).getState().setSheet(null);
      });
    });
    return () => {
      cancelAnimationFrame(frame);
    };
  }, [ui]);
  return (
    <div data-part="print" data-target="sheet" data-page="a4-portrait">
      <div data-part="print.page">{sheet}</div>
    </div>
  );
}

/** Effect: shows the print view on `#/print/<job>` and local sheets. */
export function PrintHost() {
  const hash = useHash();
  const sheet = usePrintState((s) => s.sheet);
  const jobId = jobIdOfHash(hash);
  if (jobId !== null) return createPortal(<PrintView key={jobId} jobId={jobId} />, document.body);
  if (sheet !== null) return createPortal(<SheetView sheet={sheet} />, document.body);
  return null;
}
