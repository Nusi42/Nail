/**
 * "Move to folder" (`mail.move`, `v`): a filter field over a list of the
 * active account's folders (a combobox with its listbox).
 *
 * R4 (ADR 0230): the list never moves on a guess. The arrow keys (or the
 * mouse) highlight an option and Enter takes the highlighted one only; a
 * folder whose name equals the typed text is highlighted at once. Names
 * that start with the text come before names that contain it and paths
 * that contain it. The folder the mail is already in is left out. Moving
 * into Drafts, Trash or Junk asks first in the window, with the number of
 * messages (`../features/mail-actions/move.ts`).
 */
import { useEffect, useId, useRef, useState, type KeyboardEvent, type SubmitEvent } from "react";
import { errorMessageIdOf } from "../api/client";
import type { Folder, MailTargetResolution } from "../api/generated";
import { useLabel, useT } from "../i18n/I18nProvider";
import { type WidgetProps } from "../layout/widgetRegistry";
import { useComposeContext } from "../state/hooks";
import {
  confirmedMoveRole,
  moveToFolder,
  pendingMove,
  resolveMove,
  sourceFolderOf,
  type ConfirmedMoveRole,
} from "../features/mail-actions/move";
import { useFolders } from "../state/queries";
import { useUi } from "../state/uiStore";
import { flattenFolders } from "./FolderTree";
import { folderIcon, Icon } from "./Icon";

/** Name of the layout window that holds the move picker. */
export const MOVE_WINDOW = "move";

/** How well a folder matches a filter, best first. */
export const MATCH_RANK = { exact: 0, prefix: 1, name: 2, path: 3 } as const;

/**
 * How `folder` matches the lower-cased, trimmed `needle`: its name equals
 * it, starts with it or contains it, or its path contains it; `null` when
 * it does not match.
 */
export function matchRank(folder: Pick<Folder, "name" | "path">, needle: string): number | null {
  const name = folder.name.toLowerCase();
  if (name === needle) return MATCH_RANK.exact;
  if (name.startsWith(needle)) return MATCH_RANK.prefix;
  if (name.includes(needle)) return MATCH_RANK.name;
  if (folder.path.toLowerCase().includes(needle)) return MATCH_RANK.path;
  return null;
}

const needleOf = (filter: string) => filter.trim().toLowerCase();

/** Folders whose name or path contains the filter text (case-insensitive). */
export function filterFolders(folders: readonly Folder[], filter: string): Folder[] {
  const needle = needleOf(filter);
  if (needle === "") return [...folders];
  return folders.filter((f) => matchRank(f, needle) !== null);
}

/**
 * The rows that match `filter`, best match first (exact name, name prefix,
 * name contains, path contains), in tree order within each rank; every row
 * in tree order without a filter.
 */
export function rankFolders<T extends { folder: Folder }>(rows: readonly T[], filter: string): T[] {
  const needle = needleOf(filter);
  if (needle === "") return [...rows];
  const ranked: { row: T; index: number; rank: number }[] = [];
  rows.forEach((row, index) => {
    const rank = matchRank(row.folder, needle);
    if (rank !== null) ranked.push({ row, index, rank });
  });
  return ranked.sort((a, b) => a.rank - b.rank || a.index - b.index).map((r) => r.row);
}

/**
 * The option highlighted when the filter becomes `filter` over the ranked
 * `rows`: the one folder whose name equals it (ranked first), else none —
 * Enter then moves nowhere.
 */
export function initialOption(rows: readonly { folder: Folder }[], filter: string): number | null {
  const needle = needleOf(filter);
  if (needle === "") return null;
  const exact = rows.filter((r) => matchRank(r.folder, needle) === MATCH_RANK.exact);
  return exact.length === 1 ? 0 : null;
}

/** The highlighted option after an arrow key (`null` = none yet). */
export function stepOption(active: number | null, count: number, delta: 1 | -1): number | null {
  if (count === 0) return null;
  if (active === null) return delta > 0 ? 0 : count - 1;
  return Math.min(count - 1, Math.max(0, active + delta));
}

const optionIdOf = (base: string, index: number) => `${base}-option-${String(index)}`;

/** The open question before a move into Drafts, Trash or Junk. */
interface MoveQuestion {
  folder: Folder;
  role: ConfirmedMoveRole;
  resolution: MailTargetResolution;
}

/**
 * "Move to folder": a filterable list of the active account's folders.
 * Picking one moves the window's targets there and closes the window.
 */
export function FolderPicker({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const ctx = useComposeContext();
  const accountId = useUi((s) => s.activeAccountId);
  const selectedId = useUi((s) => s.selectedMessageId);
  const closeWindow = useUi((s) => s.closeWindow);
  const folders = useFolders(accountId);
  const [filter, setFilter] = useState("");
  const [active, setActive] = useState<number | null>(null);
  const [notice, setNotice] = useState<string | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);
  const [question, setQuestion] = useState<MoveQuestion | null>(null);
  const input = useRef<HTMLInputElement>(null);
  const base = useId();
  const listId = `${base}-list`;

  const source = sourceFolderOf(ctx, pendingMove(ctx.store).targets);
  const rows = flattenFolders(folders.data ?? []).filter((r) => r.folder.id !== source);
  const visible = rankFolders(rows, filter);
  const current = active === null ? undefined : visible[active];
  const disabled = selectedId === null || busy;

  useEffect(() => {
    if (active === null) return;
    document.getElementById(optionIdOf(base, active))?.scrollIntoView({ block: "nearest" });
  }, [active, base]);

  const finish = async (folder: Folder, resolution?: MailTargetResolution) => {
    if (await moveToFolder(ctx, folder.id, resolution)) closeWindow(MOVE_WINDOW);
  };

  const pick = async (folder: Folder) => {
    if (disabled) return;
    setError(null);
    setNotice(null);
    setBusy(true);
    // P3: the conversation (or card) the window opened for; the undo
    // toast reports a failure.
    try {
      const role = confirmedMoveRole(folder);
      if (role === null) {
        await finish(folder);
        return;
      }
      const resolution = await resolveMove(ctx, folder.id);
      if (resolution.message_ids.length === 0) await finish(folder);
      else setQuestion({ folder, role, resolution });
    } catch (e) {
      setError(errorMessageIdOf(e));
    } finally {
      setBusy(false);
    }
  };

  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    if (current) void pick(current.folder);
    else if (visible.length > 0) setNotice("mail-actions-move-choose");
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (event.key !== "ArrowDown" && event.key !== "ArrowUp") return;
    event.preventDefault();
    setNotice(null);
    setActive(stepOption(active, visible.length, event.key === "ArrowDown" ? 1 : -1));
  };

  if (question !== null) {
    return (
      <MoveConfirm
        question={question}
        busy={busy}
        onMove={() => {
          setBusy(true);
          void finish(question.folder, question.resolution).finally(() => {
            setBusy(false);
          });
        }}
        onCancel={() => {
          setQuestion(null);
          queueMicrotask(() => input.current?.focus());
        }}
      />
    );
  }

  return (
    <form
      data-part="folder-picker"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("common-move-to-folder")}
      aria-busy={busy}
      onSubmit={submit}
    >
      <label data-part="folder-picker.filter">
        <span data-part="folder-picker.filter.label">{t("folder-picker-filter")}</span>
        <input
          ref={input}
          data-part="folder-picker.input"
          type="text"
          role="combobox"
          aria-expanded="true"
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={current && active !== null ? optionIdOf(base, active) : undefined}
          value={filter}
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => {
            const next = event.target.value;
            setFilter(next);
            setNotice(null);
            setActive(initialOption(rankFolders(rows, next), next));
          }}
          onKeyDown={onKeyDown}
        />
      </label>
      {selectedId === null && (
        <div data-part="folder-picker.empty">{t("folder-picker-no-selection")}</div>
      )}
      <p data-part="folder-picker.notice" role="status">
        {notice === null ? null : t(notice)}
      </p>
      {error && (
        <div data-part="folder-picker.error" role="alert">
          {t(error)}
        </div>
      )}
      {folders.isError && (
        <div data-part="folder-picker.error" role="alert">
          {t(errorMessageIdOf(folders.error))}
        </div>
      )}
      <ul
        id={listId}
        role="listbox"
        data-part="folder-picker.list"
        aria-label={t("common-folders")}
      >
        {visible.map(({ folder, depth }, index) => (
          // eslint-disable-next-line jsx-a11y/click-events-have-key-events -- the combobox input handles the keyboard for its options (aria-activedescendant).
          <li
            key={folder.id}
            id={optionIdOf(base, index)}
            role="option"
            data-part="folder-picker.item"
            data-depth={depth}
            data-state={index === active ? "active" : undefined}
            aria-selected={index === active}
            aria-disabled={disabled || undefined}
            onMouseDown={(e) => {
              // Keep the focus in the filter field.
              e.preventDefault();
            }}
            onMouseMove={() => {
              if (index !== active) setActive(index);
            }}
            onClick={() => void pick(folder)}
          >
            <span data-part="folder-picker.button" data-role={folder.role} data-folder={folder.id}>
              <Icon name={folderIcon(folder.role)} part="folder-picker.button.icon" />
              <span data-part="folder-picker.button.name">{folder.name}</span>
            </span>
          </li>
        ))}
      </ul>
      {folders.isSuccess && visible.length === 0 && (
        <div data-part="folder-picker.empty">{t("folder-picker-no-match")}</div>
      )}
    </form>
  );
}

/** "Move 3 messages to Trash?" inside the move window. */
function MoveConfirm({
  question,
  busy,
  onMove,
  onCancel,
}: {
  question: MoveQuestion;
  busy: boolean;
  onMove: () => void;
  onCancel: () => void;
}) {
  const t = useT();
  const cancel = useRef<HTMLButtonElement>(null);
  const questionId = useId();
  // The safe choice has the focus.
  useEffect(() => {
    cancel.current?.focus();
  }, []);
  return (
    <div
      data-part="mail-confirm"
      data-confirm="move"
      data-role={question.role}
      aria-busy={busy}
      role="group"
      aria-labelledby={questionId}
    >
      <p data-part="mail-confirm.question" id={questionId}>
        {t("mail-actions-move-confirm", {
          count: question.resolution.message_ids.length,
          folder: question.folder.name,
        })}
      </p>
      <p data-part="mail-confirm.note">{t(`mail-actions-move-note-${question.role}`)}</p>
      <div data-part="mail-confirm.actions">
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          disabled={busy}
          onClick={onMove}
        >
          {t("mail-actions-move-confirm-yes")}
        </button>
        <button
          ref={cancel}
          type="button"
          data-part="form.button"
          disabled={busy}
          onClick={onCancel}
        >
          {t("common-cancel")}
        </button>
      </div>
    </div>
  );
}
