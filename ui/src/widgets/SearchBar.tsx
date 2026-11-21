import {
  useEffect,
  useId,
  useMemo,
  useRef,
  useState,
  type KeyboardEvent,
  type SubmitEvent,
} from "react";
import type { SearchError } from "../api/generated";
import { useLabel, useT } from "../i18n/I18nProvider";
import { optionBool, optionString, type WidgetProps } from "../layout/widgetRegistry";
import { useCheckQuery } from "../state/queries";
import { useUi } from "../state/uiStore";
import { Icon } from "./Icon";
import { ProblemIndicator } from "./ProblemIndicator";
import { useDebounced } from "../state/debounce";
import { useLiveSearch } from "../features/search/liveSearch";

/** Fields of the query language offered as hints, with their Fluent description ids. */
export const QUERY_HINTS = [
  "from:",
  "to:",
  "subject:",
  "has:attachment",
  "is:unread",
  "is:flagged",
  "tag:",
  "account:",
  "folder:",
  "category:",
  "before:",
  "after:",
  "size>",
] as const;

/** Values offered after `is:` and `has:`. */
const VALUE_HINTS: Record<string, readonly string[]> = {
  "is:": ["is:unread", "is:read", "is:flagged", "is:unflagged", "is:answered"],
  "has:": ["has:attachment"],
};

/** Fluent id describing a hint: `search-hint-is-unread`, `search-hint-size`. */
export function hintMessageId(hint: string): string {
  return `search-hint-${hint.replace(/[:>]$/, "").replace(/[:>]/g, "-")}`;
}

/** The word being typed at the end of the query. */
function lastWord(text: string): string {
  const match = /(\S*)$/.exec(text);
  return match?.[1] ?? "";
}

/** Hints that fit what is being typed (all fields for an empty word). */
export function hintsFor(text: string): string[] {
  const word = lastWord(text)
    .replace(/^[-(]+/, "")
    .toLowerCase();
  if (word === "") return [...QUERY_HINTS];
  const values = VALUE_HINTS[word];
  if (values) return [...values];
  const colon = word.indexOf(":");
  const pool = colon > 0 ? Object.values(VALUE_HINTS).flat() : QUERY_HINTS;
  return pool.filter((hint) => hint.startsWith(word) && hint !== word);
}

/**
 * Whether typing `text` opens the hints (R2): only while a word is typed
 * that a hint completes. After a space nothing pops up; ArrowDown shows
 * every field.
 */
export function hintsWhileTyping(text: string): boolean {
  return lastWord(text) !== "" && hintsFor(text).length > 0;
}

/** Replaces the word being typed with a hint. */
export function applyHint(text: string, hint: string): string {
  const word = lastWord(text);
  const prefix = /^[-(]*/.exec(word)?.[0] ?? "";
  const complete = hint.endsWith(":") || hint.endsWith(">") ? hint : `${hint} `;
  return `${text.slice(0, text.length - word.length)}${prefix}${complete}`;
}

/**
 * The hint picked after an arrow key: `step` (+1 down, -1 up) from
 * `current` (-1: none picked), wrapping around `count` hints.
 */
export function nextHint(current: number, step: number, count: number): number {
  if (count === 0) return -1;
  if (current < 0) return step > 0 ? 0 : count - 1;
  return (((current + step) % count) + count) % count;
}

/** Delay before the query is checked while typing. */
export const CHECK_DELAY_MS = 250;

/** Where a query mistake starts and ends in `text` (clamped to it). */
export function errorSpan(text: string, error: SearchError): { start: number; end: number } {
  const start = Math.max(0, Math.min(error.start, text.length));
  return { start, end: Math.max(start, Math.min(error.end, text.length)) };
}

/**
 * The explanation of a query mistake, the details of the field's problem
 * indicator (`ProblemIndicator`, follow mode, ADR 0221). `kept` is the text
 * whose results the list still shows.
 */
function ErrorView({
  text,
  error,
  kept,
}: {
  text: string;
  error: SearchError;
  kept: string | null;
}) {
  const t = useT();
  const { start, end } = errorSpan(text, error);
  const marked = text.slice(start, end);
  return (
    <div data-part="search-bar.error" data-kind={error.kind}>
      <span data-part="search-bar.error.message">{t(`search-error-${error.kind}`)}</span>
      <span data-part="search-bar.error.position">
        {t("search-error-position", { column: start + 1 })}
      </span>
      <span data-part="search-bar.error.query" aria-hidden="true">
        <span data-part="search-bar.error.before">{text.slice(0, start)}</span>
        <mark data-part="search-bar.error.mark" data-state={marked === "" ? "empty" : undefined}>
          {marked}
        </mark>
        <span data-part="search-bar.error.after">{text.slice(end)}</span>
      </span>
      <span data-part="search-bar.error.note">
        {kept === null ? t("search-live-not-run") : t("search-live-kept", { query: kept })}
      </span>
    </div>
  );
}

/** What a search form shows and where it searches. */
export interface SearchFormProps {
  /** `data-node-id` of the layout node. */
  nodeId?: string | undefined;
  /** Accessible name of the form (localized). */
  label: string;
  /** Where a search runs: the active account, folder, or everywhere. */
  scope: string;
  showHints: boolean;
  liveCheck: boolean;
  /** The key binding that focuses the field, shown in it (M5b list header). */
  shortcut?: string | null;
  /** The same binding for `aria-keyshortcuts` (single chords only). */
  keyShortcuts?: string | undefined;
  /** Placeholder text (localized); default: an example query. */
  placeholder?: string | undefined;
}

/**
 * Search input with query-language hints and live error positions. Submitting
 * shows the results in the message list (`search` in the UI state).
 */
export function SearchBar({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  return (
    <SearchForm
      nodeId={node.id ?? undefined}
      label={node.label ? label(node.label) : t("common-search-noun")}
      scope={optionString(node, "scope", "account")}
      showHints={optionBool(node, "hints", true)}
      liveCheck={optionBool(node, "live-check", true)}
    />
  );
}

/**
 * The search form of `search-bar` and of the list header (`list-header`).
 *
 * The hints (R2, ADR 0222) are a combobox popup (WAI-ARIA combobox with a
 * listbox): they open while a word is typed that a hint completes, or with
 * ArrowDown; they float over the content under the field (the skin places
 * `search-bar.hints`) and close with Escape, Tab, a click outside the form,
 * or when the focus leaves it. Up / Down pick a hint
 * (`aria-activedescendant`), Enter or a click takes it. They never open on
 * focus alone, so a click on the list after focusing the field reaches the
 * list.
 */
export function SearchForm({
  nodeId,
  label: formLabel,
  scope,
  showHints,
  liveCheck,
  shortcut = null,
  keyShortcuts,
  placeholder,
}: SearchFormProps) {
  const t = useT();
  const ids = useId();
  const formRef = useRef<HTMLFormElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const text = useUi((s) => s.searchInput);
  const setText = useUi((s) => s.setSearchInput);
  const search = useUi((s) => s.search);
  const setSearch = useUi((s) => s.setSearch);
  const accountId = useUi((s) => s.activeAccountId);
  const folderId = useUi((s) => s.activeFolderId);
  const focusRequest = useUi((s) => s.focusRequest);
  const [focused, setFocused] = useState(false);
  const [hovered, setHovered] = useState(false);
  // The hints popup: asked for, and the hint picked with the arrow keys (-1: none).
  const [hintsOpen, setHintsOpen] = useState(false);
  const [activeHint, setActiveHint] = useState(-1);

  const searchScope = useMemo(
    () => ({
      accountId: scope === "all" ? null : accountId,
      folderId: scope === "folder" ? folderId : null,
    }),
    [scope, accountId, folderId],
  );
  const live = useLiveSearch(text, searchScope, ids);
  const debounced = useDebounced(text, CHECK_DELAY_MS);
  const check = useCheckQuery(liveCheck ? debounced : "");
  // The core's verdict on the searched query wins; then a committed text the
  // live search found invalid; else the live check of what is typed (only
  // once it has settled, to avoid flicker).
  const typed = text.trim();
  const error =
    search?.error && search.text === typed
      ? search.error
      : (live.rejected?.error ??
        (debounced === text && typed !== "" ? (check.data ?? null) : null));

  useEffect(() => {
    if (focusRequest?.target === "search") {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [focusRequest]);

  const hints = showHints && focused && hintsOpen ? hintsFor(text) : [];
  const expanded = hints.length > 0;
  const picked = activeHint >= 0 && activeHint < hints.length ? activeHint : -1;

  const closeHints = () => {
    setHintsOpen(false);
    setActiveHint(-1);
  };

  // A click outside the form closes the hints (they float over the list).
  useEffect(() => {
    if (!expanded) return;
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && formRef.current?.contains(event.target)) return;
      setHintsOpen(false);
      setActiveHint(-1);
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
    };
  }, [expanded]);

  const takeHint = (hint: string) => {
    const next = applyHint(text, hint);
    setText(next);
    setHintsOpen(hintsWhileTyping(next));
    setActiveHint(-1);
    inputRef.current?.focus();
  };

  const onInputKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (!showHints) return;
    switch (event.key) {
      case "ArrowDown":
      case "ArrowUp": {
        event.preventDefault();
        const step = event.key === "ArrowDown" ? 1 : -1;
        if (!expanded) {
          // Alt+Down only opens; Down opens and picks the first hint.
          setHintsOpen(true);
          setActiveHint(event.altKey ? -1 : nextHint(-1, step, hintsFor(text).length));
        } else {
          setActiveHint(nextHint(picked, step, hints.length));
        }
        break;
      }
      case "Enter": {
        const hint = hints[picked];
        if (hint !== undefined) {
          event.preventDefault();
          takeHint(hint);
        }
        break;
      }
      case "Escape":
        // Closes the hints only; with them closed, Escape clears the search.
        if (expanded) {
          event.preventDefault();
          event.stopPropagation();
          closeHints();
        }
        break;
      case "Tab":
        closeHints();
        break;
    }
  };

  // Enter searches at once (live search, ADR 0219).
  const submit = (event: SubmitEvent) => {
    event.preventDefault();
    closeHints();
    live.flush();
  };

  const errorId = `${ids}-error`;
  const hintsId = `${ids}-hints`;
  const optionId = (index: number) => `${ids}-hint-${String(index)}`;
  return (
    <form
      ref={formRef}
      data-part="search-bar"
      data-node-id={nodeId}
      data-key-context="search"
      data-state={search ? "active" : undefined}
      role="search"
      aria-label={formLabel}
      onSubmit={submit}
      onMouseEnter={() => {
        setHovered(true);
      }}
      onMouseLeave={() => {
        setHovered(false);
      }}
      onFocus={() => {
        setFocused(true);
        live.claim();
      }}
      onBlur={(event) => {
        const next = event.relatedTarget;
        if (!(next instanceof Node) || !event.currentTarget.contains(next)) {
          setFocused(false);
          closeHints();
        }
      }}
    >
      <div data-part="search-bar.field" data-state={error ? "invalid" : undefined}>
        <Icon name="search" part="search-bar.icon" />
        <input
          ref={inputRef}
          data-part="search-bar.input"
          data-state={error ? "invalid" : undefined}
          type="search"
          role={showHints ? "combobox" : undefined}
          value={text}
          placeholder={placeholder ?? t("search-bar-placeholder")}
          aria-label={t("common-search-mail")}
          aria-invalid={error ? true : undefined}
          aria-describedby={error ? errorId : undefined}
          aria-autocomplete={showHints ? "list" : undefined}
          aria-expanded={showHints ? expanded : undefined}
          aria-controls={expanded ? hintsId : undefined}
          aria-activedescendant={picked >= 0 ? optionId(picked) : undefined}
          aria-keyshortcuts={keyShortcuts}
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => {
            const next = event.target.value;
            setText(next);
            setHintsOpen(hintsWhileTyping(next));
            setActiveHint(-1);
          }}
          onKeyDown={onInputKeyDown}
        />
        {error && (
          // The mistake floats over the content under the field (ADR 0220,
          // 0221); the hints take its place while they are open.
          <ProblemIndicator
            severity="error"
            kind={error.kind}
            open={(focused || hovered) && !expanded}
            detailsId={errorId}
          >
            <ErrorView
              text={text}
              error={error}
              kept={search && search.text !== typed ? search.text : null}
            />
          </ProblemIndicator>
        )}
        {/* The mistake for screen readers while typing: the details only
            describe the field, which is not read again while it keeps the
            focus. The region is there before a mistake, so it is announced. */}
        {focused && (
          <span data-part="search-bar.status" role="status">
            {error
              ? t("search-error-announce", {
                  message: t(`search-error-${error.kind}`),
                  column: errorSpan(text, error).start + 1,
                })
              : ""}
          </span>
        )}
        {shortcut !== null && text === "" && !search && (
          <kbd data-part="search-bar.shortcut" aria-hidden="true">
            {shortcut}
          </kbd>
        )}
        {(text !== "" || search) && (
          <button
            type="button"
            data-part="search-bar.clear"
            aria-label={t("search-bar-clear")}
            title={t("search-bar-clear")}
            onClick={() => {
              setSearch(null);
              inputRef.current?.focus();
            }}
          >
            <Icon name="x" part="search-bar.clear.icon" />
          </button>
        )}
      </div>
      {expanded && (
        // Floats under the field (the skin); the focus stays in the input.
        <div
          data-part="search-bar.hints"
          role="presentation"
          onMouseDown={(event) => {
            event.preventDefault();
          }}
        >
          <div
            data-part="search-bar.hints.list"
            id={hintsId}
            role="listbox"
            aria-label={t("search-bar-hints")}
          >
            {hints.map((hint, index) => (
              <div
                key={hint}
                id={optionId(index)}
                data-part="search-bar.hint"
                role="option"
                aria-selected={index === picked}
                tabIndex={-1}
                onMouseDown={(event) => {
                  // Keep the focus in the field.
                  event.preventDefault();
                  takeHint(hint);
                }}
              >
                <div
                  data-part="search-bar.hint.button"
                  data-state={index === picked ? "active" : undefined}
                >
                  <code data-part="search-bar.hint.syntax">{hint}</code>
                  <span data-part="search-bar.hint.description">{t(hintMessageId(hint))}</span>
                </div>
              </div>
            ))}
          </div>
          <div data-part="search-bar.hint" data-state="info">
            <span data-part="search-bar.hint.description">{t("search-hint-operators")}</span>
          </div>
        </div>
      )}
    </form>
  );
}
