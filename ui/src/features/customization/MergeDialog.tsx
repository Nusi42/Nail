/**
 * The merge dialog (design.md, "Applying someone else's theme or layout"):
 * paste or load a shared `theme.kdl` / `layout.kdl`, compare it with yours
 * section by section, choose "keep mine" / "take theirs" per item (or per
 * section / for everything), watch a live preview, then apply. Nothing is
 * written until "Apply"; the previous file is backed up and can be restored
 * with one click. Earlier backups are listed for revert, too.
 *
 * The live preview is a temporary configuration in the renderer: the
 * resolved result of the current choices replaces the app's configuration
 * in the query cache; closing the dialog without applying restores (and
 * re-fetches) the real one.
 */
import { useQuery, useQueryClient, type QueryClient } from "@tanstack/react-query";
import { useEffect, useId, useState } from "react";
import { errorMessageIdOf, type NailClient } from "../../api/client";
import type {
  ConfigBackup,
  ConfigFile,
  ConfigMergePreview,
  MergeItem,
  MergeSectionPreview,
  MergeTake,
} from "../../api/generated";
import { normalizeLayout, normalizeTheme } from "../../api/normalize";
import { useLocale, useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { queryKeys, useClient, type AppConfig } from "../../state/queries";
import { chooseAll, choiceList, differs, summarize, takeOf, type Choices } from "./merge";
import { MERGE_FILES } from "./names";
import { DATE_TIME_SECONDS, formatDateTime } from "../../i18n/format";

/** Delay before the live preview follows a change of choices (ms). */
export const PREVIEW_DELAY_MS = 150;

export const backupsKey = (file: ConfigFile) => ["config-backups", file] as const;

function useBackups(file: ConfigFile) {
  const client = useClient();
  return useQuery({
    queryKey: backupsKey(file),
    queryFn: async () =>
      (await client.request({ type: "ListConfigBackups", file }, "ConfigBackups")).backups,
  });
}

/** Keeps the live preview in the query cache and restores the real config. */
export class LivePreview {
  private original: AppConfig | undefined;
  private active = false;

  constructor(private readonly queryClient: QueryClient) {}

  /** Shows a resolved theme and / or layout instead of the real ones. */
  show(theme: unknown, layout: unknown): void {
    const current = this.queryClient.getQueryData<AppConfig>(queryKeys.config);
    if (!this.active) this.original = current;
    const base = this.original ?? current;
    if (!base) return;
    this.active = true;
    this.queryClient.setQueryData<AppConfig>(queryKeys.config, {
      ...base,
      theme: theme ? normalizeTheme(theme) : base.theme,
      layout: layout ? normalizeLayout(layout) : base.layout,
    });
  }

  /** Puts the real configuration back (and re-fetches it to be sure). */
  restore(): void {
    if (!this.active) return;
    this.active = false;
    if (this.original) this.queryClient.setQueryData(queryKeys.config, this.original);
    void this.queryClient.invalidateQueries({ queryKey: queryKeys.config });
  }

  /** The preview became the real configuration (applied). */
  commit(): void {
    this.active = false;
  }
}

function formatTime(iso: string, locale: string): string {
  return formatDateTime(iso, locale, DATE_TIME_SECONDS);
}

function Backups({ file, client }: { file: ConfigFile; client: NailClient }) {
  const t = useT();
  const locale = useLocale();
  const queryClient = useQueryClient();
  const backups = useBackups(file);
  const [problem, setProblem] = useState<string | null>(null);
  const [done, setDone] = useState<string | null>(null);
  const revert = async (backup: ConfigBackup) => {
    setProblem(null);
    setDone(null);
    try {
      await client.request({ type: "RevertConfig", file, backup_id: backup.id }, "Done");
      setDone(backup.id);
      await queryClient.invalidateQueries({ queryKey: backupsKey(file) });
    } catch (e) {
      setProblem(errorMessageIdOf(e));
    }
  };
  const list = backups.data ?? [];
  return (
    <section data-part="merge-dialog.backups">
      <h3 data-part="merge-dialog.heading">{t("merge-dialog-backups")}</h3>
      {backups.isSuccess && list.length === 0 && (
        <p data-part="merge-dialog.none">{t("merge-dialog-no-backups")}</p>
      )}
      {problem && (
        <p data-part="merge-dialog.error" role="alert">
          {t(problem)}
        </p>
      )}
      {done && (
        <p data-part="merge-dialog.status" role="status">
          {t("merge-dialog-reverted")}
        </p>
      )}
      <ul data-part="merge-dialog.backup-list">
        {list.map((backup) => (
          <li key={backup.id} data-part="merge-dialog.backup">
            <span data-part="merge-dialog.backup.time">
              {formatTime(backup.created_at, locale)}
            </span>
            <button
              type="button"
              data-part="form.button"
              data-action="revert"
              onClick={() => void revert(backup)}
            >
              {t("merge-dialog-revert")}
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
}

interface ItemProps {
  item: MergeItem;
  take: MergeTake;
  onTake(take: MergeTake): void;
}

function ItemRow({ item, take, onTake }: ItemProps) {
  const t = useT();
  const name = useId();
  const changed = differs(item);
  return (
    <li
      data-part="merge-dialog.item"
      data-change={item.change}
      data-state={changed ? (take === "theirs" ? "theirs" : "mine") : undefined}
      data-conflict={item.conflicts.length > 0 ? "" : undefined}
    >
      <div data-part="merge-dialog.item.header">
        <code data-part="merge-dialog.item.key">{item.key}</code>
        <span data-part="merge-dialog.item.change">{t(`merge-change-${item.change}`)}</span>
        {changed && (
          <fieldset data-part="merge-dialog.choice">
            <legend data-part="merge-dialog.choice.legend">
              {t("merge-dialog-choice", { key: item.key })}
            </legend>
            <label data-part="merge-dialog.choice.option">
              <input
                type="radio"
                name={name}
                data-take="mine"
                checked={take === "mine"}
                onChange={() => onTake("mine")}
              />
              {t("merge-dialog-keep-mine")}
            </label>
            <label data-part="merge-dialog.choice.option">
              <input
                type="radio"
                name={name}
                data-take="theirs"
                checked={take === "theirs"}
                onChange={() => onTake("theirs")}
              />
              {t("merge-dialog-take-theirs")}
            </label>
          </fieldset>
        )}
      </div>
      {item.conflicts.length > 0 && (
        <ul data-part="merge-dialog.conflicts">
          {item.conflicts.map((conflict, index) => (
            <li key={index} data-part="merge-dialog.conflict" data-kind={conflict.kind}>
              <span data-part="merge-dialog.conflict.kind">
                {t(`merge-conflict-${conflict.kind}`)}
              </span>
              <span data-part="merge-dialog.conflict.detail">
                {conflict.localized
                  ? t(conflict.localized.id, conflict.localized.args)
                  : conflict.message}
              </span>
            </li>
          ))}
        </ul>
      )}
      {changed && (
        <div data-part="merge-dialog.diff">
          <div data-part="merge-dialog.side" data-side="mine">
            <span data-part="merge-dialog.side.label">{t("merge-dialog-mine")}</span>
            {item.mine === null ? (
              <span data-part="merge-dialog.side.absent">{t("merge-dialog-absent")}</span>
            ) : (
              <pre data-part="merge-dialog.side.text">{item.mine}</pre>
            )}
          </div>
          <div data-part="merge-dialog.side" data-side="theirs">
            <span data-part="merge-dialog.side.label">{t("merge-dialog-theirs")}</span>
            {item.theirs === null ? (
              <span data-part="merge-dialog.side.absent">{t("merge-dialog-absent")}</span>
            ) : (
              <pre data-part="merge-dialog.side.text">{item.theirs}</pre>
            )}
          </div>
        </div>
      )}
    </li>
  );
}

interface SectionProps {
  section: MergeSectionPreview;
  choices: Choices;
  fallback: MergeTake;
  showUnchanged: boolean;
  onTake(item: MergeItem, take: MergeTake): void;
  onTakeAll(take: MergeTake): void;
}

function SectionView({
  section,
  choices,
  fallback,
  showUnchanged,
  onTake,
  onTakeAll,
}: SectionProps) {
  const t = useT();
  const items = section.items.filter((item) => showUnchanged || differs(item));
  if (items.length === 0) return null;
  const name = t(`merge-section-${section.section}`);
  const anyDiffers = section.items.some(differs);
  return (
    <section data-part="merge-dialog.section" data-section={section.section}>
      <div data-part="merge-dialog.section.header">
        <h3 data-part="merge-dialog.heading">{name}</h3>
        {anyDiffers && (
          <>
            <button
              type="button"
              data-part="form.button"
              data-action="keep-section"
              onClick={() => onTakeAll("mine")}
            >
              {t("merge-dialog-keep-section", { section: name })}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-action="take-section"
              onClick={() => onTakeAll("theirs")}
            >
              {t("merge-dialog-take-section", { section: name })}
            </button>
          </>
        )}
      </div>
      <ul data-part="merge-dialog.items">
        {items.map((item) => (
          <ItemRow
            key={item.id}
            item={item}
            take={takeOf(choices, item, fallback)}
            onTake={(take) => onTake(item, take)}
          />
        ))}
      </ul>
    </section>
  );
}

type Stage =
  | { kind: "input" }
  | { kind: "preview"; preview: ConfigMergePreview }
  | { kind: "applied"; backup: ConfigBackup | null };

/** Dialog `config-merge`. */
export function MergeDialog({ close }: DialogProps) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const [live] = useState(() => new LivePreview(queryClient));
  const fieldId = useId();
  const [file, setFile] = useState<ConfigFile>("theme");
  const [incoming, setIncoming] = useState("");
  const [stage, setStage] = useState<Stage>({ kind: "input" });
  const [choices, setChoices] = useState<Choices>({});
  const [showUnchanged, setShowUnchanged] = useState(false);
  const [livePreview, setLivePreview] = useState(true);
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<string | null>(null);
  const fallback: MergeTake = "mine";

  // Closing without applying restores the real configuration.
  useEffect(() => () => live.restore(), [live]);

  const preview = stage.kind === "preview" ? stage.preview : null;

  // Live preview of the current choices.
  useEffect(() => {
    if (!preview || preview.errors.length > 0) return;
    if (!livePreview) {
      live.restore();
      return;
    }
    let cancelled = false;
    const timer = setTimeout(() => {
      client
        .request(
          {
            type: "ResolveConfigMerge",
            file: preview.file,
            incoming,
            choices: choiceList(choices, preview),
            default: fallback,
          },
          "ConfigMergeResolved",
        )
        .then(
          (resolved) => {
            if (!cancelled) live.show(resolved.theme, resolved.layout);
          },
          (e: unknown) => {
            if (!cancelled) setProblem(errorMessageIdOf(e));
          },
        );
    }, PREVIEW_DELAY_MS);
    return () => {
      cancelled = true;
      clearTimeout(timer);
    };
  }, [preview, choices, livePreview, incoming, client, live]);

  const compare = async () => {
    setProblem(null);
    setBusy(true);
    try {
      const r = await client.request(
        { type: "PreviewConfigMerge", file, incoming },
        "ConfigMergePreview",
      );
      setChoices({});
      setStage({ kind: "preview", preview: r.preview });
    } catch (e) {
      setProblem(errorMessageIdOf(e));
    } finally {
      setBusy(false);
    }
  };

  const apply = async () => {
    if (!preview) return;
    setProblem(null);
    setBusy(true);
    try {
      const r = await client.request(
        {
          type: "ApplyConfigMerge",
          file: preview.file,
          incoming,
          choices: choiceList(choices, preview),
          default: fallback,
        },
        "ConfigMergeApplied",
      );
      live.commit();
      setStage({ kind: "applied", backup: r.backup });
      void queryClient.invalidateQueries({ queryKey: backupsKey(preview.file) });
    } catch (e) {
      setProblem(errorMessageIdOf(e));
    } finally {
      setBusy(false);
    }
  };

  const revert = async (backup: ConfigBackup) => {
    setProblem(null);
    setBusy(true);
    try {
      await client.request(
        { type: "RevertConfig", file: backup.file, backup_id: backup.id },
        "Done",
      );
      setStage({ kind: "applied", backup: null });
      void queryClient.invalidateQueries({ queryKey: backupsKey(backup.file) });
    } catch (e) {
      setProblem(errorMessageIdOf(e));
    } finally {
      setBusy(false);
    }
  };

  const loadFile = async (files: FileList | null) => {
    const picked = files?.[0];
    if (!picked) return;
    setIncoming(await picked.text());
    if (/layout/i.test(picked.name)) setFile("layout");
    else if (/theme/i.test(picked.name)) setFile("theme");
  };

  return (
    <div data-part="merge-dialog" data-stage={stage.kind} aria-busy={busy}>
      {problem && (
        <p data-part="merge-dialog.error" role="alert">
          {t(problem)}
        </p>
      )}

      {stage.kind === "input" && (
        <form
          data-part="merge-dialog.input"
          onSubmit={(e) => {
            e.preventDefault();
            void compare();
          }}
        >
          <p data-part="merge-dialog.intro">{t("merge-dialog-intro")}</p>
          <fieldset data-part="merge-dialog.files">
            <legend data-part="merge-dialog.files.legend">{t("merge-dialog-file")}</legend>
            {MERGE_FILES.map((f) => (
              <label key={f} data-part="merge-dialog.file">
                <input
                  type="radio"
                  name={`${fieldId}-file`}
                  data-file={f}
                  checked={file === f}
                  onChange={() => setFile(f)}
                />
                {t(`merge-dialog-file-${f}`)}
              </label>
            ))}
          </fieldset>
          <label data-part="merge-dialog.upload">
            <span data-part="merge-dialog.upload.label">{t("merge-dialog-load")}</span>
            <input
              type="file"
              data-part="merge-dialog.upload.input"
              accept=".kdl,text/plain"
              onChange={(e) => void loadFile(e.target.files)}
            />
          </label>
          <label data-part="merge-dialog.paste" htmlFor={`${fieldId}-text`}>
            {t("merge-dialog-paste")}
          </label>
          <textarea
            id={`${fieldId}-text`}
            data-part="merge-dialog.text"
            value={incoming}
            spellCheck={false}
            onChange={(e) => setIncoming(e.target.value)}
          />
          <div data-part="merge-dialog.actions">
            <button
              type="submit"
              data-part="form.button"
              data-action="compare"
              disabled={busy || incoming.trim() === ""}
            >
              {t("merge-dialog-compare")}
            </button>
          </div>
          <Backups file={file} client={client} />
        </form>
      )}

      {preview && preview.errors.length > 0 && (
        <section data-part="merge-dialog.incoming-errors">
          <p data-part="merge-dialog.error" role="alert">
            {t("merge-dialog-incoming-invalid", { count: preview.errors.length })}
          </p>
          <ol data-part="merge-dialog.error-list">
            {preview.errors.map((issue, index) => (
              <li key={index} data-part="merge-dialog.error-item">
                {t("merge-dialog-error-at", {
                  line: issue.line,
                  column: issue.column,
                  message: issue.message,
                })}
              </li>
            ))}
          </ol>
          <button
            type="button"
            data-part="form.button"
            data-action="back"
            onClick={() => setStage({ kind: "input" })}
          >
            {t("merge-dialog-back")}
          </button>
        </section>
      )}

      {preview && preview.errors.length === 0 && (
        <PreviewView
          preview={preview}
          choices={choices}
          fallback={fallback}
          showUnchanged={showUnchanged}
          livePreview={livePreview}
          busy={busy}
          onChoices={setChoices}
          onShowUnchanged={setShowUnchanged}
          onLivePreview={setLivePreview}
          onBack={() => {
            live.restore();
            setStage({ kind: "input" });
          }}
          onApply={() => void apply()}
        />
      )}

      {stage.kind === "applied" && (
        <section data-part="merge-dialog.result">
          <p data-part="merge-dialog.status" role="status">
            {t(stage.backup ? "merge-dialog-applied" : "merge-dialog-reverted")}
          </p>
          <div data-part="merge-dialog.actions">
            {stage.backup && (
              <button
                type="button"
                data-part="form.button"
                data-action="revert"
                disabled={busy}
                onClick={() => stage.backup && void revert(stage.backup)}
              >
                {t("merge-dialog-revert")}
              </button>
            )}
            <button type="button" data-part="form.button" data-action="close" onClick={close}>
              {t("merge-dialog-close")}
            </button>
          </div>
        </section>
      )}
    </div>
  );
}

interface PreviewViewProps {
  preview: ConfigMergePreview;
  choices: Choices;
  fallback: MergeTake;
  showUnchanged: boolean;
  livePreview: boolean;
  busy: boolean;
  onChoices(choices: Choices): void;
  onShowUnchanged(on: boolean): void;
  onLivePreview(on: boolean): void;
  onBack(): void;
  onApply(): void;
}

function PreviewView({
  preview,
  choices,
  fallback,
  showUnchanged,
  livePreview,
  busy,
  onChoices,
  onShowUnchanged,
  onLivePreview,
  onBack,
  onApply,
}: PreviewViewProps) {
  const t = useT();
  const summary = summarize(preview, choices, fallback);
  return (
    <section data-part="merge-dialog.preview">
      <p data-part="merge-dialog.summary" role="status">
        {summary.differing === 0
          ? t("merge-dialog-identical")
          : t("merge-dialog-summary", {
              differing: summary.differing,
              taken: summary.taken,
              conflicts: summary.conflicts,
            })}
      </p>
      <div data-part="merge-dialog.toolbar">
        <button
          type="button"
          data-part="form.button"
          data-action="keep-all"
          onClick={() => onChoices(chooseAll(choices, preview, "mine"))}
        >
          {t("merge-dialog-keep-all")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-action="take-all"
          onClick={() => onChoices(chooseAll(choices, preview, "theirs"))}
        >
          {t("merge-dialog-take-all")}
        </button>
        <label data-part="merge-dialog.option">
          <input
            type="checkbox"
            data-option="live"
            checked={livePreview}
            onChange={(e) => onLivePreview(e.target.checked)}
          />
          {t("merge-dialog-live-preview")}
        </label>
        <label data-part="merge-dialog.option">
          <input
            type="checkbox"
            data-option="unchanged"
            checked={showUnchanged}
            onChange={(e) => onShowUnchanged(e.target.checked)}
          />
          {t("merge-dialog-show-unchanged")}
        </label>
      </div>
      {preview.sections.map((section) => (
        <SectionView
          key={section.section}
          section={section}
          choices={choices}
          fallback={fallback}
          showUnchanged={showUnchanged}
          onTake={(item, take) => onChoices({ ...choices, [item.id]: take })}
          onTakeAll={(take) => onChoices(chooseAll(choices, preview, take, section.section))}
        />
      ))}
      <div data-part="merge-dialog.actions">
        <button type="button" data-part="form.button" data-action="back" onClick={onBack}>
          {t("merge-dialog-back")}
        </button>
        <button
          type="button"
          data-part="form.button"
          data-action="apply"
          disabled={busy || summary.taken === 0}
          onClick={onApply}
        >
          {t("merge-dialog-apply")}
        </button>
      </div>
    </section>
  );
}
