/**
 * The theme editor's token groups (M5d WP183): every token of one group
 * (colour, type, spacing, radius, motion, more) with where its value comes
 * from (base theme, the theme it extends, this theme), a colour picker for
 * colours and the value as written in KDL for everything (`#3b5bdb`, `14`,
 * `$color.fg`, `calc(…)`). Values of one mode / variant (dark, high
 * contrast, print, compact) are edited with the editor's mode switch.
 */
import { useId, useState, type KeyboardEvent } from "react";
import type { ThemeTokenValue } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { Icon } from "../../widgets/Icon";
import { Tag } from "../../widgets/primitives";
import type { ThemeEdits } from "./useThemeEdits";
import {
  draftKey,
  hasModeValue,
  pickerColor,
  resolveTokenValues,
  tokenGroup,
  tokenId,
  valueIn,
  type TokenGroup,
} from "./tokens";

interface RowProps {
  token: ThemeTokenValue;
  mode: string | null;
  editable: boolean;
  edits: ThemeEdits;
  /** The token resolved to CSS (for the colour picker of `$refs`). */
  resolved: string | undefined;
  /** The Appearance override that wins over this token, if any. */
  override: string | undefined;
  /** Removes that override. */
  onClearOverride: () => void;
}

/** The value text field: commits on Enter or when it loses the focus. */
function ValueField({
  initial,
  placeholder,
  label,
  disabled,
  onCommit,
}: {
  initial: string;
  placeholder: string;
  label: string;
  disabled: boolean;
  onCommit: (value: string) => void;
}) {
  const [text, setText] = useState(initial);
  // A new saved value (a write, Undo) replaces the text; the field stays
  // (and keeps the focus) instead of being mounted anew.
  const [seen, setSeen] = useState(initial);
  if (seen !== initial) {
    setSeen(initial);
    setText(initial);
  }
  const commit = () => {
    if (text !== initial) onCommit(text);
  };
  return (
    <input
      type="text"
      data-part="form.input"
      data-field="token-value"
      // Nothing typed: Ctrl+Z / Ctrl+Y undo / redo the theme (ADR 0138).
      data-history-keys={text === initial ? "" : undefined}
      aria-label={label}
      spellCheck={false}
      value={text}
      placeholder={placeholder}
      readOnly={disabled}
      onChange={(event) => {
        setText(event.target.value);
      }}
      onBlur={commit}
      onKeyDown={(event: KeyboardEvent<HTMLInputElement>) => {
        if (event.key === "Enter") {
          event.preventDefault();
          commit();
        } else if (event.key === "Escape" && text !== initial) {
          event.preventDefault();
          event.stopPropagation();
          setText(initial);
        }
      }}
    />
  );
}

function TokenRow({ token, mode, editable, edits, resolved, override, onClearOverride }: RowProps) {
  const t = useT();
  const id = tokenId(token.category, token.name);
  const key = draftKey(token.category, token.name, mode);
  const own = hasModeValue(token, mode);
  const draft = edits.drafts.get(key);
  const shown = draft === undefined ? (own ? valueIn(token, mode) : "") : (draft ?? "");
  const placeholder = valueIn(token, null);
  const color = token.category === "color" ? pickerColor(shown || (resolved ?? "")) : null;
  const write = (value: string | null, now: boolean) => {
    edits.change(
      key,
      { op: "set-token", category: token.category, name: token.name, value, mode },
      value,
      now,
    );
  };
  const removable = editable && token.origin === "own" && own && draft !== null;
  return (
    <li
      data-part="theme-editor.token"
      data-token={id}
      data-origin={token.origin}
      data-state={draft === undefined ? undefined : "changed"}
    >
      <span data-part="theme-editor.token.text">
        <code data-part="theme-editor.token.name">{id}</code>
        <Tag text={t(`theme-editor-origin-${token.origin}`)} kind="origin" />
        {override !== undefined && (
          <>
            <Tag text={t("theme-editor-overridden", { value: override })} kind="override" />
            <button
              type="button"
              data-part="form.button"
              data-variant="ghost"
              onClick={onClearOverride}
            >
              {t("theme-editor-clear-override")}
            </button>
          </>
        )}
      </span>
      <span data-part="theme-editor.token.controls">
        {token.category === "color" && (
          <input
            type="color"
            data-part="theme-editor.token.color"
            aria-label={t("theme-editor-token-pick", { token: id })}
            value={color ?? "#000000"}
            disabled={!editable}
            onChange={(event) => {
              write(event.target.value, false);
            }}
          />
        )}
        <ValueField
          initial={shown}
          placeholder={placeholder}
          label={t("theme-editor-token-value", { token: id })}
          disabled={!editable}
          onCommit={(text) => {
            const value = text.trim();
            if (value === "") {
              if (own && token.origin === "own") write(null, true);
              return;
            }
            write(value, true);
          }}
        />
        {removable && (
          <button
            type="button"
            data-part="form.button"
            data-variant="icon"
            aria-label={t("theme-editor-token-reset", { token: id })}
            title={t("theme-editor-token-reset", { token: id })}
            onClick={() => {
              write(null, true);
            }}
          >
            <Icon name="rotate-ccw" part="form.button.icon" />
          </button>
        )}
      </span>
    </li>
  );
}

/** One group's tokens, filtered by the search text. */
export function TokenList({
  tokens,
  group,
  mode,
  editable,
  edits,
  overrides = new Map(),
  onClearOverride = () => undefined,
}: {
  tokens: readonly ThemeTokenValue[];
  group: TokenGroup;
  mode: string | null;
  editable: boolean;
  edits: ThemeEdits;
  /** Appearance overrides by token id (they win over the theme). */
  overrides?: ReadonlyMap<string, string>;
  /** Removes the override of a token id. */
  onClearOverride?: (token: string) => void;
}) {
  const t = useT();
  const searchId = useId();
  const [filter, setFilter] = useState("");
  const resolved = resolveTokenValues(tokens, mode, edits.drafts);
  const needle = filter.trim().toLowerCase();
  const shown = tokens.filter(
    (token) =>
      tokenGroup(token.category) === group &&
      (needle === "" || tokenId(token.category, token.name).includes(needle)),
  );
  return (
    <div data-part="theme-editor.tokens" data-group={group}>
      <div data-part="form.field" data-field="token-filter">
        <label data-part="form.label" htmlFor={searchId}>
          {t("theme-editor-token-filter")}
        </label>
        <input
          id={searchId}
          type="search"
          data-part="form.input"
          value={filter}
          onChange={(event) => {
            setFilter(event.target.value);
          }}
        />
      </div>
      {shown.length === 0 ? (
        <p data-part="theme-editor.empty">{t("theme-editor-no-tokens")}</p>
      ) : (
        <ul data-part="theme-editor.token-list" aria-label={t(`theme-editor-group-${group}`)}>
          {shown.map((token) => (
            <TokenRow
              key={tokenId(token.category, token.name)}
              token={token}
              mode={mode}
              editable={editable}
              edits={edits}
              resolved={resolved.get(tokenId(token.category, token.name))}
              override={overrides.get(tokenId(token.category, token.name))}
              onClearOverride={() => {
                onClearOverride(tokenId(token.category, token.name));
              }}
            />
          ))}
        </ul>
      )}
    </div>
  );
}
