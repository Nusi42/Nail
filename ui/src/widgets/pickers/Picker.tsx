/**
 * The shared picker (R2, ADR 0222): a text field with a browse / search /
 * select list of the values its kind offers (a folder tree across
 * accounts, accounts, tags, addresses and contacts, mailing lists, header
 * names). Free text stays allowed: typing changes the value at once, the
 * list only helps to find one.
 *
 * ARIA combobox pattern: the input is the `combobox`; its popup is a
 * `listbox` (a `tree` for folders) that the input points into with
 * `aria-activedescendant`, so the focus stays in the field. Keys: Down /
 * Alt+Down open the list and move, Up moves back, Home / End, Right opens a
 * folder row or enters it, Left closes it or goes to its parent, Enter picks
 * (or opens an account row), Escape and Tab close it; a click outside closes
 * it too. The popup overlays the content below (`picker.popup` is
 * positioned in `picker.field`), it never moves it.
 *
 * Fields whose value is an id (accounts, folders with `folderValue="id"`)
 * show the option's name; text typed that names exactly one option is taken
 * as that option when the field loses the focus.
 *
 * Parts (`PICKER_PARTS`, looks in `defaults/theme.kdl`): `picker`
 * (`data-kind`, `data-state="open"`; the host may name another root part
 * with `part`), `picker.field`, `picker.input`, `picker.browse`,
 * `picker.popup` (`data-placement`: `below`, or `above` when the room is
 * above the field), `picker.list`, `picker.option` (`data-depth`,
 * `data-selected`, `data-variant`, state `highlighted`), `picker.option.toggle`
 * (`data-expanded`), `picker.option.icon`, `picker.option.label`,
 * `picker.option.detail`, `picker.empty`; the search field's values
 * (`./searchValues.tsx`) render `picker.option.button` rows.
 */
import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { useT } from "../../i18n/I18nProvider";
import { Icon } from "../Icon";
import { optionKey, usePickerOptions } from "./sources";
import {
  initialActive,
  moveActive,
  parentKeys,
  pickable,
  toggled,
  visibleRows,
  type TreeMove,
} from "./tree";
import type { PickerKind, PickerOption, PickerProps } from "./types";

/** Every part a picker renders (the theme styles these). */
export const PICKER_PARTS = [
  "picker",
  "picker.field",
  "picker.input",
  "picker.browse",
  "picker.popup",
  "picker.list",
  "picker.option",
  "picker.option.button",
  "picker.option.toggle",
  "picker.option.icon",
  "picker.option.label",
  "picker.option.detail",
  "picker.empty",
] as const;

/**
 * The popup's tallest size (px) the theme allows (`picker.popup`
 * `max-height`); with less room below the field and more above it, the popup
 * opens above (`data-placement="above"`).
 */
export const POPUP_ROOM = 320;

/** Where the popup opens for a field at `rect` in a viewport `height` px tall. */
export function popupPlacement(
  rect: { top: number; bottom: number },
  height: number,
): "below" | "above" {
  const below = height - rect.bottom;
  return below < POPUP_ROOM && rect.top > below ? "above" : "below";
}

/** Whether a picker's value is an id that the field shows by its option's name. */
export function showsLabel(kind: PickerKind, folderValue: PickerProps["folderValue"]): boolean {
  return kind === "account" || (kind === "folder" && folderValue === "id");
}

/** The one option whose name is `text` (case-insensitive), if exactly one is. */
export function optionNamed(options: readonly PickerOption[], text: string): PickerOption | null {
  const needle = text.trim().toLowerCase();
  if (needle === "") return null;
  const named = options.filter(
    (o) => pickable(o) && o.variant !== "create" && o.label.toLowerCase() === needle,
  );
  return named.length === 1 ? (named[0] ?? null) : null;
}

/** The icon of an option row. */
function optionIcon(kind: PickerKind, option: PickerOption): string | null {
  if (option.variant === "create") return "plus";
  if (option.folder) return option.folder.role === "other" ? "folder" : option.folder.role;
  if (option.variant === "group" || kind === "account") return "user";
  return null;
}

const MOVES: Partial<Record<string, TreeMove>> = {
  ArrowDown: "next",
  ArrowUp: "previous",
  Home: "first",
  End: "last",
};

export function Picker({
  kind,
  value,
  onChange,
  label,
  accountId = null,
  folderValue = "path",
  virtualAccounts = false,
  id,
  placeholder,
  disabled,
  invalid,
  describedBy,
  part,
}: PickerProps) {
  const t = useT();
  const ids = useId();
  const listId = `${ids}-list`;
  const rootRef = useRef<HTMLSpanElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const [open, setOpen] = useState(false);
  /** Text typed since the field last showed its value (`null` = not typing). */
  const [typed, setTyped] = useState<string | null>(null);
  /** Whether the list filters by what was typed (opened by typing) or shows everything. */
  const [filtering, setFiltering] = useState(false);
  const [active, setActive] = useState<string | null>(null);
  const [collapsed, setCollapsed] = useState<ReadonlySet<string>>(new Set());
  const [placement, setPlacement] = useState<"below" | "above">("below");
  const scope = { accountId, folderValue, virtualAccounts };
  const named = showsLabel(kind, folderValue);
  // Every option: the list while browsing, and the name of an id value (or of typed text).
  const all = usePickerOptions({
    kind,
    query: "",
    scope,
    enabled: (open && !filtering) || (named && (value !== "" || typed !== null)),
  });
  const filtered = usePickerOptions({
    kind,
    query: typed ?? "",
    scope,
    enabled: open && filtering,
  });
  const listed = filtering ? filtered : all;
  const rows = visibleRows(listed.options, collapsed, filtering);
  const parents = parentKeys(listed.options);
  const current =
    active !== null && rows.some((r) => optionKey(r) === active)
      ? active
      : filtering
        ? initialActive(rows, null, typed)
        : initialActive(rows, value);
  const shownValue = named
    ? (all.options.find((o) => pickable(o) && o.value === value)?.label ?? value)
    : value;
  const display = typed ?? shownValue;
  const tree = kind === "folder";
  const rowId = (key: string) => `${listId}-${String(rows.findIndex((r) => optionKey(r) === key))}`;

  const close = () => {
    setOpen(false);
    setActive(null);
  };

  /** Leaves typing: an id field takes the option the typed text names. */
  const settle = () => {
    if (typed !== null && named) {
      const option = optionNamed(all.options, typed);
      if (option) onChange(option.value, option);
    }
    setTyped(null);
  };

  const pick = (option: PickerOption) => {
    if (!pickable(option)) {
      setCollapsed((c) => toggled(c, optionKey(option)));
      setActive(optionKey(option));
      return;
    }
    onChange(option.value, option);
    setTyped(null);
    setFiltering(false);
    close();
  };

  const browse = () => {
    setFiltering(false);
    setActive(null);
    setOpen(true);
  };

  useLayoutEffect(() => {
    const input = inputRef.current;
    if (open && input)
      setPlacement(popupPlacement(input.getBoundingClientRect(), window.innerHeight));
  }, [open]);

  useEffect(() => {
    if (!open) return;
    const outside = (event: MouseEvent) => {
      if (rootRef.current && event.target instanceof Node && rootRef.current.contains(event.target))
        return;
      setOpen(false);
      setActive(null);
    };
    document.addEventListener("mousedown", outside);
    return () => {
      document.removeEventListener("mousedown", outside);
    };
  }, [open]);

  useEffect(() => {
    if (!open || current === null) return;
    const index = rows.findIndex((r) => optionKey(r) === current);
    const row = document.getElementById(`${listId}-${String(index)}`);
    row?.scrollIntoView({ block: "nearest" });
  });

  const onKeyDown = (event: KeyboardEvent<HTMLInputElement>) => {
    const move = MOVES[event.key];
    if (event.key === "ArrowDown" && (!open || event.altKey)) {
      event.preventDefault();
      if (!open) browse();
      return;
    }
    if (event.key === "ArrowUp" && event.altKey) {
      event.preventDefault();
      close();
      return;
    }
    if (!open) return;
    // While typing, Home / End stay with the text.
    if (move && !(filtering && (move === "first" || move === "last"))) {
      event.preventDefault();
      setActive(moveActive(rows, current, move));
    } else if (
      tree &&
      current !== null &&
      (event.key === "ArrowRight" || event.key === "ArrowLeft")
    ) {
      const isParent = parents.has(current);
      const isClosed = !filtering && collapsed.has(current);
      event.preventDefault();
      if (event.key === "ArrowRight") {
        if (isParent && isClosed) setCollapsed((c) => toggled(c, current));
        else setActive(moveActive(rows, current, "child"));
      } else if (isParent && !isClosed && !filtering) {
        setCollapsed((c) => toggled(c, current));
      } else {
        setActive(moveActive(rows, current, "parent"));
      }
    } else if (event.key === "Enter") {
      const option = rows.find((r) => optionKey(r) === current);
      if (option) {
        event.preventDefault();
        pick(option);
      }
    } else if (event.key === "Escape") {
      // Closes the list, not the window around it.
      event.preventDefault();
      event.stopPropagation();
      close();
    } else if (event.key === "Tab") {
      close();
    }
  };

  const empty = listed.loading
    ? "pickers-loading"
    : kind === "address" && (typed ?? "").trim() === ""
      ? "pickers-type-to-search"
      : "pickers-no-match";

  return (
    <span
      ref={rootRef}
      data-part={part ?? "picker"}
      data-kind={kind}
      data-state={open ? "open" : undefined}
    >
      <span data-part="picker.field">
        <input
          ref={inputRef}
          data-part="picker.input"
          type="text"
          id={id}
          role="combobox"
          value={display}
          aria-label={label}
          placeholder={placeholder}
          disabled={disabled}
          aria-invalid={invalid === true ? true : undefined}
          aria-describedby={describedBy}
          aria-autocomplete="list"
          aria-haspopup={tree ? "tree" : "listbox"}
          aria-expanded={open}
          aria-controls={listId}
          aria-activedescendant={open && current !== null ? rowId(current) : undefined}
          autoComplete="off"
          spellCheck={false}
          onKeyDown={onKeyDown}
          onBlur={() => {
            settle();
            close();
          }}
          onChange={(event) => {
            const text = event.target.value;
            setTyped(text);
            setFiltering(true);
            setActive(null);
            setOpen(true);
            onChange(text, null);
          }}
        />
        <button
          type="button"
          data-part="picker.browse"
          tabIndex={-1}
          disabled={disabled}
          aria-label={t("pickers-browse", { field: label })}
          title={t("pickers-browse", { field: label })}
          aria-expanded={open}
          aria-controls={listId}
          // Keep the focus in the field.
          onMouseDown={(event) => {
            event.preventDefault();
          }}
          onClick={() => {
            if (open) close();
            else browse();
            inputRef.current?.focus();
          }}
        >
          <Icon name="chevron-down" part="picker.browse.icon" />
        </button>
        {open && (
          <span data-part="picker.popup" data-placement={placement}>
            <ul
              id={listId}
              data-part="picker.list"
              role={tree ? "tree" : "listbox"}
              aria-label={t("pickers-list", { field: label })}
            >
              {rows.map((option, index) => {
                const key = optionKey(option);
                const selected =
                  pickable(option) && option.variant !== "create" && option.value === value;
                const isParent = parents.has(key);
                const expanded = isParent ? filtering || !collapsed.has(key) : undefined;
                const icon = optionIcon(kind, option);
                return (
                  // eslint-disable-next-line jsx-a11y/click-events-have-key-events, jsx-a11y/no-noninteractive-element-interactions -- an option / tree item (role set per kind); its keys are the combobox input's (aria-activedescendant).
                  <li
                    key={key}
                    id={`${listId}-${String(index)}`}
                    data-part="picker.option"
                    data-depth={option.depth}
                    data-selected={selected ? "true" : undefined}
                    data-variant={option.variant}
                    data-state={key === current ? "highlighted" : undefined}
                    role={tree ? "treeitem" : "option"}
                    aria-selected={selected}
                    aria-level={tree ? option.depth + 1 : undefined}
                    aria-expanded={tree ? expanded : undefined}
                    // Keep the focus in the field; a click picks (or opens an account row).
                    onMouseDown={(event) => {
                      event.preventDefault();
                    }}
                    onClick={(event) => {
                      // The chevron opens or closes a row with children; the rest picks it.
                      const onToggle =
                        event.target instanceof Element &&
                        event.target.closest('[data-part="picker.option.toggle"]') !== null;
                      if (onToggle && isParent && pickable(option)) {
                        setCollapsed((c) => toggled(c, key));
                        setActive(key);
                      } else {
                        pick(option);
                      }
                    }}
                  >
                    {tree && (
                      <span
                        data-part="picker.option.toggle"
                        data-expanded={expanded === undefined ? undefined : String(expanded)}
                        aria-hidden="true"
                      >
                        {isParent && (
                          <Icon
                            name={expanded === true ? "chevron-down" : "chevron-right"}
                            part="picker.option.toggle.icon"
                          />
                        )}
                      </span>
                    )}
                    {icon !== null && <Icon name={icon} part="picker.option.icon" />}
                    <span data-part="picker.option.label">{option.label}</span>
                    {option.detail !== null && (
                      <span data-part="picker.option.detail">{option.detail}</span>
                    )}
                  </li>
                );
              })}
            </ul>
            {rows.length === 0 && (
              <span data-part="picker.empty" role="status">
                {t(empty)}
              </span>
            )}
          </span>
        )}
      </span>
    </span>
  );
}
