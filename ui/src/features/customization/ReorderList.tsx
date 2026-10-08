/**
 * An ordered list the keyboard can rearrange: each entry has "Move up",
 * "Move down" and "Remove" buttons, and Alt+Up / Alt+Down move the focused
 * entry. Focus follows a moved entry, so a keyboard user can keep moving
 * it. Used by the toolbar and column editors.
 */
import { useEffect, useRef, useState, type KeyboardEvent } from "react";
import { useT } from "../../i18n/I18nProvider";
import { RemoveButton } from "../../widgets/RemoveButton";

export interface ReorderEntry {
  /** Stable key (survives moves). */
  key: string;
  /** Visible text of the entry. */
  label: string;
  /** Extra `data-kind` for the theme (separator, spacer, …). */
  kind?: string;
}

export interface ReorderListProps {
  entries: readonly ReorderEntry[];
  /** Accessible name of the list. */
  label: string;
  /** `data-part` prefix, e.g. `toolbar-editor`. */
  part: string;
  onMove: (from: number, to: number) => void;
  onRemove: (index: number) => void;
}

type Button = "up" | "down" | "remove" | "entry";

export function ReorderList({ entries, label, part, onMove, onRemove }: ReorderListProps) {
  const t = useT();
  const refs = useRef(new Map<string, HTMLElement>());
  const [focus, setFocus] = useState<{ key: string; button: Button } | null>(null);

  useEffect(() => {
    if (!focus) return;
    const target =
      refs.current.get(`${focus.key}:${focus.button}`) ?? refs.current.get(`${focus.key}:entry`);
    target?.focus();
  }, [focus, entries]);

  const move = (index: number, delta: number, button: Button) => {
    const to = index + delta;
    const entry = entries[index];
    if (!entry || to < 0 || to >= entries.length) return;
    onMove(index, to);
    // At the ends the pressed button disables itself: keep focus on the entry then.
    const atEnd = (delta < 0 && to === 0) || (delta > 0 && to === entries.length - 1);
    setFocus({ key: entry.key, button: atEnd && button !== "entry" ? "entry" : button });
  };

  const remove = (index: number) => {
    const next = entries[index + 1] ?? entries[index - 1];
    onRemove(index);
    if (next) setFocus({ key: next.key, button: "entry" });
  };

  const onKeyDown = (event: KeyboardEvent<HTMLLIElement>, index: number) => {
    if (!event.altKey || (event.key !== "ArrowUp" && event.key !== "ArrowDown")) return;
    event.preventDefault();
    event.stopPropagation();
    move(index, event.key === "ArrowUp" ? -1 : 1, "entry");
  };

  const setRef = (key: string) => (el: HTMLElement | null) => {
    if (el) refs.current.set(key, el);
    else refs.current.delete(key);
  };

  return (
    <ol data-part={`${part}.items`} aria-label={label}>
      {entries.map((entry, index) => (
        // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- the item takes the focus programmatically so Alt+arrow keys can move it; its buttons do the same by pointer.
        <li
          key={entry.key}
          ref={setRef(`${entry.key}:entry`)}
          data-part={`${part}.item`}
          data-kind={entry.kind}
          tabIndex={-1}
          aria-label={t("customize-reorder-position", {
            label: entry.label,
            position: index + 1,
            count: entries.length,
          })}
          onKeyDown={(event) => {
            onKeyDown(event, index);
          }}
        >
          <span data-part={`${part}.item.label`}>{entry.label}</span>
          <button
            ref={setRef(`${entry.key}:up`)}
            type="button"
            data-part="form.button"
            data-action="move-up"
            disabled={index === 0}
            aria-label={t("customize-move-up", { label: entry.label })}
            onClick={() => {
              move(index, -1, "up");
            }}
          >
            {t("common-move-up")}
          </button>
          <button
            ref={setRef(`${entry.key}:down`)}
            type="button"
            data-part="form.button"
            data-action="move-down"
            disabled={index === entries.length - 1}
            aria-label={t("customize-move-down", { label: entry.label })}
            onClick={() => {
              move(index, 1, "down");
            }}
          >
            {t("common-move-down")}
          </button>
          <RemoveButton
            label={t("customize-remove", { label: entry.label })}
            onRemove={() => {
              remove(index);
            }}
          />
        </li>
      ))}
    </ol>
  );
}

/** `items` with the entry at `from` moved to `to`. */
export function moved<T>(items: readonly T[], from: number, to: number): T[] {
  const out = [...items];
  const [item] = out.splice(from, 1);
  if (item !== undefined) out.splice(to, 0, item);
  return out;
}
