/**
 * A recipient field (To / Cc / Bcc) with address autocomplete from the
 * core's collected addresses (`SuggestAddresses`). The field stays a plain
 * text box of comma-separated addresses; suggestions complete the entry
 * being typed.
 */
import { useQuery } from "@tanstack/react-query";
import { useEffect, useId, useState, type KeyboardEvent } from "react";
import type { Address } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { formatAddress } from "../../state/addresses";
import { useClient } from "../../state/queries";

/** How many suggestions are shown. */
export const SUGGESTION_LIMIT = 8;
/** Typing pause before asking the core. */
export const SUGGEST_DELAY_MS = 150;

/** The entry being typed: text after the last separator outside quotes / brackets. */
export function currentEntry(value: string): { start: number; text: string } {
  let quoted = false;
  let angle = false;
  let start = 0;
  for (let i = 0; i < value.length; i++) {
    const c = value[i];
    if (c === '"') quoted = !quoted;
    else if (c === "<" && !quoted) angle = true;
    else if (c === ">" && !quoted) angle = false;
    else if ((c === "," || c === ";") && !quoted && !angle) start = i + 1;
  }
  return { start, text: value.slice(start).trim() };
}

/** `value` with the entry being typed replaced by `address`. */
export function completeEntry(value: string, address: Address): string {
  const { start } = currentEntry(value);
  const head = value.slice(0, start).trimEnd();
  return `${head}${head === "" ? "" : " "}${formatAddress(address)}, `;
}

function useDebounced<T>(value: T, ms: number): T {
  const [debounced, setDebounced] = useState(value);
  useEffect(() => {
    const timer = setTimeout(() => {
      setDebounced(value);
    }, ms);
    return () => {
      clearTimeout(timer);
    };
  }, [value, ms]);
  return debounced;
}

export interface AddressFieldProps {
  field: "to" | "cc" | "bcc";
  value: string;
  onChange: (value: string) => void;
}

export function AddressField({ field, value, onChange }: AddressFieldProps) {
  const t = useT();
  const client = useClient();
  const id = useId();
  const listId = `${id}-suggestions`;
  const [focused, setFocused] = useState(false);
  const [active, setActive] = useState(0);
  const [closed, setClosed] = useState(false);
  const prefix = useDebounced(currentEntry(value).text, SUGGEST_DELAY_MS);
  const enabled = focused && prefix.length > 0 && !closed;
  const suggestions = useQuery({
    queryKey: ["composer", "suggest", prefix],
    queryFn: async () =>
      (
        await client.request(
          { type: "SuggestAddresses", prefix, limit: SUGGESTION_LIMIT },
          "AddressSuggestions",
        )
      ).addresses,
    enabled,
    staleTime: 30_000,
    retry: false,
  });
  const typed = value.toLowerCase();
  const options = enabled
    ? (suggestions.data ?? []).filter((a) => !typed.includes(`<${a.email.toLowerCase()}>`))
    : [];
  const open = options.length > 0;
  const current = Math.min(active, Math.max(0, options.length - 1));

  const choose = (address: Address) => {
    onChange(completeEntry(value, address));
    setActive(0);
    setClosed(true);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    if (!open) return;
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActive((current + 1) % options.length);
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setActive((current - 1 + options.length) % options.length);
    } else if (event.key === "Enter" || event.key === "Tab") {
      const address = options[current];
      if (address && !event.ctrlKey && !event.metaKey) {
        event.preventDefault();
        choose(address);
      }
    } else if (event.key === "Escape") {
      // Closes the list, not the window.
      event.preventDefault();
      setClosed(true);
    }
  };

  return (
    <div data-part="composer.field" data-field={field}>
      <label data-part="composer.field.label" htmlFor={id}>
        {t(`composer-field-${field}`)}
      </label>
      <div data-part="composer.address">
        <input
          id={id}
          data-part="composer.field.input"
          type="text"
          value={value}
          autoComplete="off"
          spellCheck={false}
          aria-autocomplete="list"
          aria-controls={open ? listId : undefined}
          aria-activedescendant={open ? `${listId}-${String(current)}` : undefined}
          onFocus={() => {
            setFocused(true);
          }}
          onBlur={() => {
            setFocused(false);
          }}
          onKeyDown={onKeyDown}
          onChange={(event) => {
            setClosed(false);
            setActive(0);
            onChange(event.target.value);
          }}
        />
        {open && (
          <ul
            id={listId}
            data-part="composer.suggestions"
            role="listbox"
            aria-label={t("composer-suggestions-label")}
          >
            {options.map((address, index) => (
              <li
                key={address.email}
                id={`${listId}-${String(index)}`}
                data-part="composer.suggestion"
                data-state={index === current ? "active" : undefined}
                role="option"
                aria-selected={index === current}
                onMouseDown={(event) => {
                  // Keep the focus in the field.
                  event.preventDefault();
                  choose(address);
                }}
              >
                {address.name && <span data-part="composer.suggestion.name">{address.name}</span>}
                <span data-part="composer.suggestion.email">{address.email}</span>
              </li>
            ))}
          </ul>
        )}
      </div>
    </div>
  );
}
