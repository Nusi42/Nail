import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import type { SearchError } from "../api/generated";
import { useLabel, useT } from "../i18n/I18nProvider";
import { optionBool, optionString, type WidgetProps } from "../layout/widgetRegistry";
import { useCheckQuery } from "../state/queries";
import { useUi } from "../state/uiStore";
import { Icon } from "./Icon";

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

/** Replaces the word being typed with a hint. */
export function applyHint(text: string, hint: string): string {
  const word = lastWord(text);
  const prefix = /^[-(]*/.exec(word)?.[0] ?? "";
  const complete = hint.endsWith(":") || hint.endsWith(">") ? hint : `${hint} `;
  return `${text.slice(0, text.length - word.length)}${prefix}${complete}`;
}

/** Delay before the query is checked while typing. */
export const CHECK_DELAY_MS = 250;

function useDebounced(value: string, ms: number): string {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => setDebounced(value), ms);
    return () => clearTimeout(timer);
  }, [value, ms]);
  return debounced;
}

function ErrorView({ id, text, error }: { id: string; text: string; error: SearchError }) {
  const t = useT();
  const start = Math.max(0, Math.min(error.start, text.length));
  const end = Math.max(start, Math.min(error.end, text.length));
  const marked = text.slice(start, end);
  return (
    <div data-part="search-bar.error" id={id} role="alert" data-kind={error.kind}>
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
      label={node.label ? label(node.label) : t("search-bar-label")}
      scope={optionString(node, "scope", "account")}
      showHints={optionBool(node, "hints", true)}
      liveCheck={optionBool(node, "live-check", true)}
    />
  );
}

/** The search form of `search-bar` and of the list header (`list-header`). */
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
  const inputRef = useRef<HTMLInputElement>(null);
  const text = useUi((s) => s.searchInput);
  const setText = useUi((s) => s.setSearchInput);
  const search = useUi((s) => s.search);
  const setSearch = useUi((s) => s.setSearch);
  const accountId = useUi((s) => s.activeAccountId);
  const folderId = useUi((s) => s.activeFolderId);
  const focusRequest = useUi((s) => s.focusRequest);
  const [focused, setFocused] = useState(false);

  const debounced = useDebounced(text, CHECK_DELAY_MS);
  const check = useCheckQuery(liveCheck ? debounced : "");
  // The core's verdict on the submitted query wins; else the live check of
  // what is typed (only once it has settled, to avoid flicker).
  const error =
    search?.error && search.text === text
      ? search.error
      : debounced === text && text.trim() !== ""
        ? (check.data ?? null)
        : null;

  useEffect(() => {
    if (focusRequest?.target === "search") {
      inputRef.current?.focus();
      inputRef.current?.select();
    }
  }, [focusRequest]);

  const hints = showHints && focused ? hintsFor(text) : [];

  const submit = (event: FormEvent) => {
    event.preventDefault();
    const query = text.trim();
    if (query === "") {
      setSearch(null);
      return;
    }
    setSearch({
      text: query,
      accountId: scope === "all" ? null : accountId,
      folderId: scope === "folder" ? folderId : null,
      error: null,
    });
  };

  const errorId = `${ids}-error`;
  const hintsId = `${ids}-hints`;
  return (
    <form
      data-part="search-bar"
      data-node-id={nodeId}
      data-key-context="search"
      data-state={search ? "active" : undefined}
      role="search"
      aria-label={formLabel}
      onSubmit={submit}
      // Hints hide only when focus leaves the whole form, so Tab reaches them
      // (M5b review: hiding on the input's blur dropped focus to <body>).
      onFocus={() => setFocused(true)}
      onBlur={(event) => {
        const next = event.relatedTarget;
        if (!(next instanceof Node) || !event.currentTarget.contains(next)) setFocused(false);
      }}
    >
      <div data-part="search-bar.field">
        <Icon name="search" part="search-bar.icon" />
        <input
          ref={inputRef}
          data-part="search-bar.input"
          data-state={error ? "invalid" : undefined}
          type="search"
          value={text}
          placeholder={placeholder ?? t("search-bar-placeholder")}
          aria-label={t("search-bar-input")}
          aria-invalid={error ? true : undefined}
          aria-describedby={error ? errorId : undefined}
          aria-controls={hints.length > 0 ? hintsId : undefined}
          aria-keyshortcuts={keyShortcuts}
          autoComplete="off"
          spellCheck={false}
          onChange={(event) => setText(event.target.value)}
        />
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
      {error && <ErrorView id={errorId} text={text} error={error} />}
      {hints.length > 0 && (
        <ul data-part="search-bar.hints" id={hintsId} aria-label={t("search-bar-hints")}>
          {hints.map((hint) => (
            <li key={hint} data-part="search-bar.hint">
              <button
                type="button"
                data-part="search-bar.hint.button"
                // Keep the focus in the input while picking a hint.
                onMouseDown={(event) => event.preventDefault()}
                onClick={() => {
                  setText(applyHint(text, hint));
                  inputRef.current?.focus();
                }}
              >
                <code data-part="search-bar.hint.syntax">{hint}</code>
                <span data-part="search-bar.hint.description">{t(hintMessageId(hint))}</span>
              </button>
            </li>
          ))}
          <li data-part="search-bar.hint" data-state="info">
            <span data-part="search-bar.hint.description">{t("search-hint-operators")}</span>
          </li>
        </ul>
      )}
    </form>
  );
}
