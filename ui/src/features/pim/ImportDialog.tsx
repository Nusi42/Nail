/**
 * Importing `.ics` / `.vcf` files (dialog `pim-import`): choose a file, see
 * what it holds (events, tasks, contacts, groups and the first titles, plus
 * problems the core found), choose the calendar / address book, import.
 * The file's text goes to the core as uploaded data (`PimImportSource::
 * Data`); a file Nail was opened with (a launch argument, desktop only) is
 * read by the core (`PimImportSource::File`) and previewed the same way.
 */
import { useEffect, useEffectEvent, useId, useState, type ChangeEvent } from "react";
import type {
  LocalizedMessage,
  PimImportPreview,
  PimImportResult,
  PimImportSource,
} from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { Problem, problemOf, useChangeContext, BookOptions } from "./common";
import { pimWrite, useAddressBooks, useCalendars } from "./queries";
import { usePim, usePimStore } from "./store";
import { nonEmpty } from "../../text/empty";

/** What the dialog imports: uploaded text or a desktop file. */
interface ImportFile {
  name: string;
  source: PimImportSource;
}

/** The file name of a path (`C:\x\a.ics` → `a.ics`). */
function baseName(path: string): string {
  return nonEmpty(path.split(/[\\/]/).pop()) ?? path;
}

/** Largest file the dialog reads (bytes). */
export const MAX_IMPORT_BYTES = 10 * 1024 * 1024;

export function ImportDialog({ close }: DialogProps) {
  const t = useT();
  const ctx = useChangeContext();
  const pim = usePimStore();
  const ids = { file: useId(), calendar: useId(), book: useId() };
  const calendars = useCalendars();
  const books = useAddressBooks();
  const [file, setFile] = useState<ImportFile | null>(null);
  const [preview, setPreview] = useState<PimImportPreview | null>(null);
  const [result, setResult] = useState<PimImportResult | null>(null);
  const [calendarId, setCalendarId] = useState("");
  const [bookId, setBookId] = useState("");
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const [busy, setBusy] = useState(false);
  const writableCalendars = (calendars.data ?? []).filter((c) => !c.read_only);
  const writableBooks = (books.data ?? []).filter((b) => !b.read_only);

  const choose = async (event: ChangeEvent<HTMLInputElement>) => {
    const chosen = event.target.files?.[0];
    setPreview(null);
    setResult(null);
    setProblem(null);
    if (!chosen) return;
    if (chosen.size > MAX_IMPORT_BYTES) {
      setProblem({ id: "pim-import-too-large" });
      return;
    }
    const content = await chosen.text();
    await load({
      name: chosen.name,
      source: { type: "data", file_name: chosen.name, content },
    });
  };

  const load = async (next: ImportFile) => {
    setBusy(true);
    try {
      setFile(next);
      const answer = await ctx.client.request(
        { type: "PreviewPimImport", source: next.source },
        "PimImportPreview",
      );
      setPreview(answer.preview);
      setCalendarId(
        writableCalendars.find((c) =>
          c.components.includes(answer.preview.events > 0 ? "event" : "task"),
        )?.id ?? "",
      );
      setBookId(writableBooks.find((b) => b.role === null)?.id ?? writableBooks[0]?.id ?? "");
    } catch (error) {
      setProblem(problemOf(error));
    } finally {
      setBusy(false);
    }
  };

  // A file Nail was opened with (taken when the dialog opens): previewed
  // at once.
  const takeLaunchPath = usePim((s) => s.setImportPath);
  const [launchPath] = useState(() => pim.getState().importPath);
  const previewLaunchPath = useEffectEvent((path: string) => {
    takeLaunchPath(null);
    void Promise.resolve().then(() =>
      load({ name: baseName(path), source: { type: "file", path } }),
    );
  });
  useEffect(() => {
    if (launchPath !== null) previewLaunchPath(launchPath);
  }, [launchPath]);

  const run = async () => {
    if (!file || !preview) return;
    setBusy(true);
    setProblem(null);
    try {
      const answer = await pimWrite(
        ctx,
        {
          type: "ImportPim",
          source: file.source,
          calendar_id: preview.events + preview.tasks > 0 ? calendarId || null : null,
          address_book_id: preview.contacts + preview.groups > 0 ? bookId || null : null,
        },
        "PimImported",
      );
      setResult(answer.result);
    } catch (error) {
      setProblem(problemOf(error));
    } finally {
      setBusy(false);
    }
  };

  return (
    <div data-part="pim.import" aria-busy={busy}>
      <p data-part="pim.import.intro">{t("pim-import-intro")}</p>
      <div data-part="form.field" data-field="file">
        <label data-part="form.label" htmlFor={ids.file}>
          {t("common-file")}
        </label>
        <input
          id={ids.file}
          data-part="form.file"
          type="file"
          accept=".ics,.vcf,.vcard,text/calendar,text/vcard,text/x-vcard"
          onChange={(e) => void choose(e)}
        />
      </div>
      {preview && (
        <section data-part="pim.import.preview" aria-label={t("pim-import-preview")}>
          <p data-part="pim.import.summary">
            {t("pim-import-summary", {
              name: nonEmpty(preview.name) ?? nonEmpty(file?.name) ?? "none",
              events: preview.events,
              tasks: preview.tasks,
              contacts: preview.contacts,
              groups: preview.groups,
            })}
          </p>
          {preview.sample.length > 0 && (
            <ul data-part="pim.import.sample" aria-label={t("pim-import-sample")}>
              {preview.sample.map((title, i) => (
                <li key={i} data-part="pim.import.sample.item">
                  {title}
                </li>
              ))}
            </ul>
          )}
          {preview.issues.length > 0 && (
            <ul data-part="pim.import.issues" aria-label={t("pim-import-issues")}>
              {preview.issues.map((issue, i) => (
                <li key={i} data-part="pim.import.issue">
                  {issue.item
                    ? t("pim-import-issue-item", {
                        message: t(issue.localized.id, issue.localized.args),
                        item: issue.item,
                      })
                    : t(issue.localized.id, issue.localized.args)}
                </li>
              ))}
            </ul>
          )}
          {preview.events + preview.tasks > 0 && (
            <div data-part="form.field" data-field="calendar">
              <label data-part="form.label" htmlFor={ids.calendar}>
                {t("pim-import-calendar")}
              </label>
              <select
                id={ids.calendar}
                data-part="form.select"
                value={calendarId}
                onChange={(e) => {
                  setCalendarId(e.target.value);
                }}
              >
                {writableCalendars.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.settings.name}
                  </option>
                ))}
              </select>
            </div>
          )}
          {preview.contacts + preview.groups > 0 && (
            <div data-part="form.field" data-field="address-book">
              <label data-part="form.label" htmlFor={ids.book}>
                {t("pim-import-address-book")}
              </label>
              <select
                id={ids.book}
                data-part="form.select"
                value={bookId}
                onChange={(e) => {
                  setBookId(e.target.value);
                }}
              >
                <BookOptions books={writableBooks} />
              </select>
            </div>
          )}
        </section>
      )}
      {result && (
        <p data-part="pim.import.result" role="status">
          {t("pim-import-result", {
            created: result.created,
            updated: result.updated,
            skipped: result.skipped,
          })}
        </p>
      )}
      <Problem message={problem} />
      <div data-part="pim.editor.actions">
        {preview && !result && (
          <button
            type="button"
            data-part="form.button"
            data-variant="primary"
            disabled={busy}
            onClick={() => void run()}
          >
            {t("common-import")}
          </button>
        )}
        <button type="button" data-part="form.button" onClick={close}>
          {result ? t("common-close") : t("common-cancel")}
        </button>
      </div>
    </div>
  );
}
