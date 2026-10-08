/**
 * The command palette (dialog `command-palette`, action `palette.open`,
 * default keys Ctrl+K / Ctrl+Shift+P from `layout.kdl`): every registered
 * action — built-ins, features, the layout's aliases and plug-in commands
 * — with its localized title, its key bindings and whether it can run here
 * (`buildPalette`), searched fuzzily over titles and keywords, recently
 * used ones first (kept by the core: `GetRecentActions` /
 * `RecordActionUsed`).
 *
 * Keyboard-first (WAI-ARIA combobox with a listbox): type to filter,
 * ↑ / ↓ / Page Up / Page Down / Home / End to move, Enter to run, Escape
 * to close. Unavailable actions are listed last and dimmed; choosing one
 * says why nothing happens instead of running it. Running closes the
 * palette first, so focus returns to where the user was and the action
 * sees that context.
 *
 * This module imports the app registry; `index.ts` loads it lazily so the
 * registry and this feature never import each other while loading.
 */
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { Fragment, useEffect, useId, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useActions } from "../../actions/ActionsProvider";
import { buildPalette, type ActionInfo, type PaletteEntry } from "../../actions/palette";
import type { ActionAvailability, RecentActions } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useLayoutConfig } from "../../layout/LayoutContext";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { ariaShortcut, keyLabel } from "../menus/keyLabels";
import { appRegistry } from "../registry";
import { useInstallation } from "../system/queries";
import { paletteInfos, pluginTitles } from "./infos";
import { contextsOf } from "./contexts";
import { searchPalette, type Range, type ScoredEntry } from "./fuzzy";
import { availabilityMessageId } from "./messages";
import { paletteStoreFor, usePaletteState } from "./store";

/** Query key of the recent actions. */
export const RECENT_KEY = ["palette", "recent"] as const;

/** Rows a Page Up / Page Down moves. */
const PAGE = 8;

/** The recently used actions (kept by the core). */
export function useRecentActions() {
  const client = useClient();
  return useQuery({
    queryKey: RECENT_KEY,
    retry: false,
    queryFn: async () =>
      (await client.request({ type: "GetRecentActions" }, "RecentActions")).recent,
  });
}

/** The title with the matched parts marked. */
/** The element id of result row `index` in the palette with id `base`. */
function optionId(base: string, index: number): string {
  return `${base}-option-${String(index)}`;
}

function Highlighted({ text, ranges }: { text: string; ranges: readonly Range[] }) {
  if (ranges.length === 0) return <>{text}</>;
  const pieces: { text: string; match: boolean }[] = [];
  let at = 0;
  for (const [start, end] of ranges) {
    if (start > at) pieces.push({ text: text.slice(at, start), match: false });
    pieces.push({ text: text.slice(start, end), match: true });
    at = end;
  }
  if (at < text.length) pieces.push({ text: text.slice(at), match: false });
  return (
    <>
      {pieces.map((p, i) =>
        p.match ? (
          <mark key={i} data-part="palette.match">
            {p.text}
          </mark>
        ) : (
          <Fragment key={i}>{p.text}</Fragment>
        ),
      )}
    </>
  );
}

function Shortcuts({ keys }: { keys: readonly string[] }) {
  const t = useT();
  if (keys.length === 0) return null;
  return (
    <span data-part="palette.shortcuts">
      {keys.map((key) => (
        <kbd key={key} data-part="palette.shortcut">
          {keyLabel(key, t)}
        </kbd>
      ))}
    </span>
  );
}

function Row({
  scored,
  id,
  active,
  firstOther,
  onRun,
  onHover,
}: {
  scored: ScoredEntry;
  id: string;
  active: boolean;
  firstOther: boolean;
  onRun: () => void;
  onHover: () => void;
}) {
  const t = useT();
  const { entry } = scored;
  const shortcut = entry.shortcuts[0];
  return (
    // eslint-disable-next-line jsx-a11y/click-events-have-key-events -- the combobox input handles the keyboard for its options (aria-activedescendant).
    <li
      id={id}
      role="option"
      data-part="palette.item"
      data-action={entry.id}
      data-source={entry.source}
      data-state={
        [
          active ? "active" : "",
          entry.available ? "" : "unavailable",
          entry.recentRank !== null ? "recent" : "",
        ]
          .filter(Boolean)
          .join(" ") || undefined
      }
      data-first-unavailable={firstOther ? "" : undefined}
      aria-selected={active}
      aria-disabled={!entry.available || undefined}
      aria-keyshortcuts={shortcut ? ariaShortcut(shortcut) : undefined}
      onMouseDown={(e) => {
        e.preventDefault();
      }}
      onMouseMove={onHover}
      onClick={onRun}
    >
      <span data-part="palette.item.title">
        <Highlighted text={entry.title} ranges={scored.ranges} />
      </span>
      {entry.source === "plugin" && (
        <span data-part="palette.item.badge" data-badge="plugin">
          {t("plugin-log-source-plugin")}
        </span>
      )}
      {entry.recentRank !== null && (
        <span data-part="palette.item.badge" data-badge="recent">
          {t("palette-recent")}
        </span>
      )}
      {!entry.available && (
        <span data-part="palette.item.unavailable">{t("palette-unavailable")}</span>
      )}
      <Shortcuts keys={entry.shortcuts} />
    </li>
  );
}

/** Why an unavailable action did not run: the contexts it needs. */
function needsText(info: ActionInfo | undefined, t: ReturnType<typeof useT>): string {
  const wanted = (info?.availability ?? []).filter((a) => a !== "global");
  if (wanted.length === 0) return t("palette-not-now");
  return t("palette-needs", { places: wanted.map((a) => t(availabilityMessageId(a))).join(", ") });
}

/** The palette's content. */
export function CommandPalette({ close }: { close: () => void }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const ui = useUiStoreApi();
  const layout = useLayoutConfig();
  const dispatcher = useActions();
  const recent = useRecentActions();
  const installation = useInstallation();
  const captured = usePaletteState((s) => s.contexts);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const [notice, setNotice] = useState<string | null>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLUListElement>(null);
  const base = useId();
  const listId = `${base}-list`;

  const infos = useMemo(() => paletteInfos(), []);
  const contexts = useMemo(() => {
    const set = new Set<ActionAvailability>(captured ?? contextsOf(ui.getState(), false));
    if (installation.isSuccess) set.add("desktop");
    return set;
  }, [captured, ui, installation.isSuccess]);

  const entries: PaletteEntry[] = useMemo(() => {
    const plugins = pluginTitles(layout);
    return buildPalette({
      actionIds: [...Object.keys(appRegistry.actions), ...Object.keys(plugins)],
      infos,
      pluginTitles: plugins,
      layout,
      contexts,
      recent: recent.data?.actions ?? [],
      t,
      enabled: (id) => dispatcher.isEnabled(id),
    });
  }, [layout, infos, contexts, recent.data, t, dispatcher]);

  const results = useMemo(() => searchPalette(entries, query), [entries, query]);
  const current = Math.min(active, Math.max(0, results.length - 1));
  const firstUnavailable = results.findIndex((r) => !r.entry.available);

  // Keep the active row in view.
  useEffect(() => {
    const row = listRef.current?.querySelector<HTMLElement>(
      `#${CSS.escape(optionId(base, current))}`,
    );
    row?.scrollIntoView({ block: "nearest" });
    // `results`: a new result list moves the rows under the active index.
  }, [base, current, results]);

  const run = async (scored: ScoredEntry | undefined) => {
    if (!scored) return;
    const { entry } = scored;
    if (!entry.available) {
      setNotice(needsText(infos[entry.id], t));
      return;
    }
    paletteStoreFor(ui).getState().setLastRun(entry.id);
    close();
    await dispatcher.dispatch(entry.id, entry.args);
    try {
      const answer = await client.request(
        { type: "RecordActionUsed", action_id: entry.id },
        "RecentActions",
      );
      queryClient.setQueryData<RecentActions>(RECENT_KEY, answer.recent);
    } catch {
      // Remembering is a convenience; the action already ran.
    }
  };

  const clearRecent = async () => {
    const answer = await client.request({ type: "ClearRecentActions" }, "RecentActions");
    queryClient.setQueryData<RecentActions>(RECENT_KEY, answer.recent);
    inputRef.current?.focus();
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    const last = results.length - 1;
    const move = (to: number) => {
      event.preventDefault();
      setNotice(null);
      setActive(Math.max(0, Math.min(last, to)));
    };
    switch (event.key) {
      case "ArrowDown": {
        move(current >= last ? 0 : current + 1);
        return;
      }
      case "ArrowUp": {
        move(current <= 0 ? last : current - 1);
        return;
      }
      case "PageDown": {
        move(current + PAGE);
        return;
      }
      case "PageUp": {
        move(current - PAGE);
        return;
      }
      case "Home":
        if (event.ctrlKey) {
          move(0);
          return;
        }
        return;
      case "End":
        if (event.ctrlKey) {
          move(last);
          return;
        }
        return;
      case "Enter":
        event.preventDefault();
        void run(results[current]);
        return;
    }
  };

  const hasRecent = (recent.data?.actions.length ?? 0) > 0;
  return (
    <div data-part="palette">
      <div data-part="palette.search">
        <input
          ref={inputRef}
          data-part="palette.input"
          type="text"
          role="combobox"
          aria-expanded="true"
          aria-controls={listId}
          aria-autocomplete="list"
          aria-activedescendant={results.length > 0 ? optionId(base, current) : undefined}
          aria-label={t("palette-search-label")}
          placeholder={t("palette-search-placeholder")}
          autoComplete="off"
          spellCheck={false}
          value={query}
          onChange={(e) => {
            setQuery(e.target.value);
            setActive(0);
            setNotice(null);
          }}
          onKeyDown={onKeyDown}
        />
      </div>
      <p data-part="palette.status" role="status" aria-live="polite">
        {notice ?? t("palette-count", { count: results.length })}
      </p>
      {results.length === 0 ? (
        <p data-part="palette.empty">{t("palette-no-match", { query })}</p>
      ) : (
        <ul
          ref={listRef}
          id={listId}
          role="listbox"
          data-part="palette.list"
          aria-label={t("common-actions")}
        >
          {results.map((scored, index) => (
            <Row
              key={scored.entry.id}
              id={optionId(base, index)}
              scored={scored}
              active={index === current}
              firstOther={index === firstUnavailable && index > 0}
              onHover={() => {
                setActive(index);
              }}
              onRun={() => void run(scored)}
            />
          ))}
        </ul>
      )}
      <div data-part="palette.footer">
        <span data-part="palette.hint">{t("palette-keys-hint")}</span>
        {hasRecent && (
          <button
            type="button"
            data-part="form.button"
            data-action="clear-recent"
            onClick={() => void clearRecent()}
          >
            {t("palette-clear-recent")}
          </button>
        )}
      </div>
    </div>
  );
}

export function CommandPaletteDialog({ close }: DialogProps) {
  return <CommandPalette close={close} />;
}
