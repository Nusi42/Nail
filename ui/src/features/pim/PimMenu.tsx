/**
 * A menu button (WAI-ARIA menu button pattern) for the calendar list's
 * per-calendar "…" menus and the "+" menus of its groups (WP164). It looks
 * like the app's context menus (the shared `context-menu.*` parts), but
 * runs local commands instead of layout actions.
 *
 * Keyboard: Enter / Space / Down on the button open the menu on its first
 * item (Up: on the last); in the menu Up / Down / Home / End move, Enter /
 * Space choose, Escape closes and returns to the button, Tab closes. A
 * click elsewhere closes it. The popup is positioned below the button
 * through a geometry rule (`useGeometry`), never inline styles.
 */
import { useEffect, useId, useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";
import { useGeometry } from "../../theme/ThemeProvider";
import { Icon } from "../../widgets/Icon";
import { Swatch } from "./common";

/** One entry of a menu. Texts come in localized. */
export type PimMenuEntry =
  | {
      type: "item";
      id: string;
      label: string;
      icon?: string;
      /** A colour square instead of an icon (colour choices). */
      color?: string;
      /** A choice (`menuitemradio`): whether it is the current one. */
      checked?: boolean;
      danger?: boolean;
      onSelect(): void;
    }
  | { type: "header"; id: string; label: string }
  | { type: "separator"; id: string };

type Item = Extract<PimMenuEntry, { type: "item" }>;

export interface PimMenuProps {
  /** Accessible name of the button (and the menu). */
  label: string;
  /** Lucide icon of the button. */
  icon: string;
  entries: readonly PimMenuEntry[];
  /** `data-field` of the button (tests, theme rules). */
  field?: string;
  /** Part of the button (default `pim.menu-button`). */
  part?: string;
}

export function PimMenu({ label, icon, entries, field, part = "pim.menu-button" }: PimMenuProps) {
  const id = useId();
  const geom = `pim-menu-${id.replace(/[^a-zA-Z0-9_-]/g, "_")}`;
  const buttonRef = useRef<HTMLButtonElement>(null);
  const popupRef = useRef<HTMLDivElement>(null);
  const itemRefs = useRef<(HTMLButtonElement | null)[]>([]);
  const [open, setOpen] = useState<"first" | "last" | null>(null);
  const [position, setPosition] = useState({ x: 0, y: 0 });
  useGeometry(geom, { left: `${position.x}px`, top: `${position.y}px` });

  const items = entries
    .map((entry, index) => ({ entry, index }))
    .filter((e): e is { entry: Item; index: number } => e.entry.type === "item");

  const close = (refocus: boolean) => {
    setOpen(null);
    if (refocus) buttonRef.current?.focus();
  };

  // Place the popup under the button, inside the window.
  useLayoutEffect(() => {
    if (open === null) return;
    const button = buttonRef.current?.getBoundingClientRect();
    const popup = popupRef.current?.getBoundingClientRect();
    if (!button) return;
    const width = popup?.width ?? 0;
    const height = popup?.height ?? 0;
    const x = Math.max(0, Math.min(button.left, window.innerWidth - width));
    const below = button.bottom + 4;
    const y = below + height > window.innerHeight ? Math.max(0, button.top - height - 4) : below;
    setPosition((p) => (p.x === x && p.y === y ? p : { x, y }));
  }, [open]);

  // Focus the first / last item when the menu opens.
  useEffect(() => {
    if (open === null) return;
    const target = open === "first" ? items[0] : items[items.length - 1];
    if (target) itemRefs.current[target.index]?.focus();
    // Only when the menu opens.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  // A click outside closes the menu.
  useEffect(() => {
    if (open === null) return;
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (!(target instanceof Node)) return;
      if (popupRef.current?.contains(target) || buttonRef.current?.contains(target)) return;
      setOpen(null);
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    return () => document.removeEventListener("pointerdown", onPointerDown, true);
  }, [open]);

  const onButtonKeyDown = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === "ArrowDown" || event.key === "Enter" || event.key === " ") {
      event.preventDefault();
      setOpen("first");
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setOpen("last");
    }
  };

  const onItemKeyDown = (event: KeyboardEvent<HTMLButtonElement>, index: number) => {
    const at = items.findIndex((e) => e.index === index);
    const focus = (i: number) => {
      const target = items[(i + items.length) % items.length];
      if (target) itemRefs.current[target.index]?.focus();
    };
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        focus(at + 1);
        break;
      case "ArrowUp":
        event.preventDefault();
        focus(at - 1);
        break;
      case "Home":
        event.preventDefault();
        focus(0);
        break;
      case "End":
        event.preventDefault();
        focus(items.length - 1);
        break;
      case "Escape":
        event.preventDefault();
        event.stopPropagation();
        close(true);
        break;
      case "Tab":
        setOpen(null);
        break;
    }
  };

  return (
    <>
      <button
        ref={buttonRef}
        type="button"
        data-part={part}
        data-field={field}
        data-state={open !== null ? "open" : undefined}
        aria-label={label}
        title={label}
        aria-haspopup="menu"
        aria-expanded={open !== null}
        aria-controls={open !== null ? `${id}-menu` : undefined}
        onClick={() => (open !== null ? close(false) : setOpen("first"))}
        onKeyDown={onButtonKeyDown}
      >
        <Icon name={icon} part={`${part}.icon`} />
      </button>
      {open !== null && (
        <div
          ref={popupRef}
          data-part="context-menu"
          data-geom={geom}
          data-key-context="menu"
          data-key-modal=""
        >
          <div id={`${id}-menu`} data-part="context-menu.list" role="menu" aria-label={label}>
            {entries.map((entry, index) => {
              if (entry.type === "separator") {
                return <div key={entry.id} data-part="context-menu.separator" role="separator" />;
              }
              if (entry.type === "header") {
                return (
                  <div key={entry.id} data-part="context-menu.header" role="presentation">
                    {entry.label}
                  </div>
                );
              }
              const radio = entry.checked !== undefined;
              return (
                <button
                  key={entry.id}
                  ref={(el) => {
                    itemRefs.current[index] = el;
                  }}
                  type="button"
                  data-part="context-menu.item"
                  data-item={entry.id}
                  data-tone={entry.danger ? "danger" : undefined}
                  data-state={entry.checked ? "checked" : undefined}
                  role={radio ? "menuitemradio" : "menuitem"}
                  aria-checked={radio ? entry.checked : undefined}
                  tabIndex={-1}
                  onClick={() => {
                    close(true);
                    entry.onSelect();
                  }}
                  onPointerEnter={(event) => event.currentTarget.focus()}
                  onKeyDown={(event) => onItemKeyDown(event, index)}
                >
                  {entry.color !== undefined ? (
                    <Swatch
                      geom={`${geom}-${entry.id}`}
                      color={entry.color}
                      part="context-menu.item.swatch"
                    />
                  ) : (
                    <Icon name={entry.icon} part="context-menu.item.icon" />
                  )}
                  <span data-part="context-menu.item.label">{entry.label}</span>
                  {entry.checked && <Icon name="check" part="context-menu.item.check" />}
                </button>
              );
            })}
          </div>
        </div>
      )}
    </>
  );
}
