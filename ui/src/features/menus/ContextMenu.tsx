/**
 * A context menu (WAI-ARIA menu pattern): items are `menuitem`s with a
 * roving focus — Up / Down / Home / End move, Enter / Space run, Right
 * opens a submenu and Left closes it, Escape closes (focus goes back to
 * where it was), Tab closes, a letter jumps to the next item starting with
 * it. Disabled actions stay focusable but do nothing (`aria-disabled`).
 *
 * The menu is positioned through the theme's stylesheet (`useGeometry`),
 * never with inline styles.
 */
import {
  useEffect,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import type { JsonValue, ResolvedMenuEntry } from "../../api/generated";
import { useActions } from "../../actions/ActionsProvider";
import { useLabel, useT } from "../../i18n/I18nProvider";
import { useGeometry } from "../../theme/ThemeProvider";
import { Icon } from "../../widgets/Icon";
import { ariaShortcut, keyLabel } from "./keyLabels";

export interface ContextMenuProps {
  items: readonly ResolvedMenuEntry[];
  /** Accessible name (the target's kind). */
  label: string;
  x: number;
  y: number;
  /** Focus the first item at once (opened with the keyboard). */
  autoFocus: boolean;
  /** Runs an action, then the host closes the menu. */
  onRun(action: string, args: readonly JsonValue[]): void;
  /** Closes the menu; `restoreFocus` for keyboard closing. */
  onClose(restoreFocus: boolean): void;
}

type Navigable = Exclude<ResolvedMenuEntry, { type: "separator" }>;

function entryLabel(entry: Navigable, label: (l: string) => string): string {
  if (entry.type === "submenu") return label(entry.label);
  return label(entry.label ?? entry.action);
}

interface ListProps {
  items: readonly ResolvedMenuEntry[];
  label: string;
  level: number;
  autoFocus: boolean;
  onRun(action: string, args: readonly JsonValue[]): void;
  onClose(restoreFocus: boolean): void;
  /** Closes this submenu and focuses its parent item (level > 0). */
  onBack?: () => void;
}

function MenuList({ items, label: name, level, autoFocus, onRun, onClose, onBack }: ListProps) {
  const t = useT();
  const label = useLabel();
  const { isEnabled } = useActions();
  const refs = useRef<(HTMLButtonElement | null)[]>([]);
  const [openSub, setOpenSub] = useState<number | null>(null);
  const [subFocus, setSubFocus] = useState(false);
  const ids = useId();

  const navigable = items
    .map((entry, index) => ({ entry, index }))
    .filter((e): e is { entry: Navigable; index: number } => e.entry.type !== "separator");

  useEffect(() => {
    if (!autoFocus) return;
    const first = navigable[0];
    if (first) refs.current[first.index]?.focus();
    // Only when the list appears.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const focusAt = (position: number) => {
    const count = navigable.length;
    if (count === 0) return;
    const target = navigable[((position % count) + count) % count];
    if (target) refs.current[target.index]?.focus();
  };

  const positionOf = (index: number) => navigable.findIndex((n) => n.index === index);

  const activate = (entry: Navigable, index: number, byKeyboard: boolean) => {
    if (entry.type === "submenu") {
      setOpenSub(index);
      setSubFocus(byKeyboard);
      return;
    }
    if (!isEnabled(entry.action, entry.args)) return;
    onRun(entry.action, entry.args);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLElement>, entry: Navigable, index: number) => {
    const at = positionOf(index);
    let handled = true;
    switch (event.key) {
      case "ArrowDown":
        focusAt(at + 1);
        break;
      case "ArrowUp":
        focusAt(at - 1);
        break;
      case "Home":
        focusAt(0);
        break;
      case "End":
        focusAt(navigable.length - 1);
        break;
      case "ArrowRight":
        if (entry.type === "submenu") activate(entry, index, true);
        break;
      case "ArrowLeft":
        if (onBack) onBack();
        break;
      case "Enter":
      case " ":
        activate(entry, index, true);
        break;
      case "Escape":
        if (onBack) onBack();
        else onClose(true);
        break;
      case "Tab":
        onClose(true);
        break;
      default: {
        const letter = event.key.length === 1 ? event.key.toLocaleLowerCase() : null;
        if (letter === null || event.ctrlKey || event.altKey || event.metaKey) {
          handled = false;
          break;
        }
        for (let step = 1; step <= navigable.length; step++) {
          const candidate = navigable[(at + step) % navigable.length];
          if (
            candidate &&
            entryLabel(candidate.entry, label).toLocaleLowerCase().startsWith(letter)
          ) {
            refs.current[candidate.index]?.focus();
            break;
          }
        }
      }
    }
    if (handled) {
      // Key bindings of the app must not see keys meant for the menu.
      event.preventDefault();
      event.stopPropagation();
    }
  };

  return (
    <div
      data-part="context-menu.list"
      data-level={level}
      role="menu"
      aria-label={name}
      aria-orientation="vertical"
    >
      {items.map((entry, index): ReactNode => {
        if (entry.type === "separator") {
          return <div key={index} data-part="context-menu.separator" role="separator" />;
        }
        const text = entryLabel(entry, label);
        if (entry.type === "submenu") {
          const expanded = openSub === index;
          return (
            <div key={index} data-part="context-menu.submenu" role="none">
              <button
                ref={(el) => {
                  refs.current[index] = el;
                }}
                type="button"
                data-part="context-menu.item"
                data-state={expanded ? "expanded" : undefined}
                role="menuitem"
                tabIndex={-1}
                aria-haspopup="menu"
                aria-expanded={expanded}
                aria-controls={expanded ? `${ids}-sub-${index}` : undefined}
                onClick={() => activate(entry, index, false)}
                onPointerEnter={(event) => event.currentTarget.focus()}
                onKeyDown={(event) => onKeyDown(event, entry, index)}
              >
                <span data-part="context-menu.item.label">{text}</span>
                <Icon name="chevron-right" part="context-menu.item.arrow" />
              </button>
              {expanded && (
                <div id={`${ids}-sub-${index}`} data-part="context-menu.popup">
                  <MenuList
                    items={entry.items}
                    label={text}
                    level={level + 1}
                    autoFocus={subFocus}
                    onRun={onRun}
                    onClose={onClose}
                    onBack={() => {
                      setOpenSub(null);
                      refs.current[index]?.focus();
                    }}
                  />
                </div>
              )}
            </div>
          );
        }
        const enabled = isEnabled(entry.action, entry.args);
        const shortcut = entry.shortcut;
        return (
          <button
            key={index}
            ref={(el) => {
              refs.current[index] = el;
            }}
            type="button"
            data-part="context-menu.item"
            data-action={entry.action}
            data-state={enabled ? undefined : "disabled"}
            role="menuitem"
            tabIndex={-1}
            aria-disabled={enabled ? undefined : true}
            aria-keyshortcuts={shortcut ? ariaShortcut(shortcut) : undefined}
            onClick={() => activate(entry, index, false)}
            onPointerEnter={(event) => {
              event.currentTarget.focus();
              setOpenSub(null);
            }}
            onKeyDown={(event) => onKeyDown(event, entry, index)}
          >
            <Icon name={entry.icon} part="context-menu.item.icon" />
            <span data-part="context-menu.item.label">{text}</span>
            {shortcut && (
              <span data-part="context-menu.item.shortcut" aria-hidden="true">
                {keyLabel(shortcut, t)}
              </span>
            )}
          </button>
        );
      })}
    </div>
  );
}

/** The menu at a viewport position; closes on a click elsewhere. */
export function ContextMenu({ items, label, x, y, autoFocus, onRun, onClose }: ContextMenuProps) {
  const geom = useId();
  const ref = useRef<HTMLDivElement>(null);
  const [position, setPosition] = useState({ x, y });
  useGeometry(geom, { left: `${position.x}px`, top: `${position.y}px` });

  // Keep the menu inside the window.
  useLayoutEffect(() => {
    const el = ref.current;
    if (!el) return;
    const rect = el.getBoundingClientRect();
    const maxX = Math.max(0, window.innerWidth - rect.width);
    const maxY = Math.max(0, window.innerHeight - rect.height);
    const next = { x: Math.min(x, maxX), y: Math.min(y, maxY) };
    if (rect.width > 0 && (next.x !== x || next.y !== y)) setPosition(next);
  }, [x, y]);

  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      if (event.target instanceof Node && ref.current?.contains(event.target)) return;
      onClose(false);
    };
    const onBlur = () => onClose(false);
    document.addEventListener("pointerdown", onPointerDown, true);
    window.addEventListener("blur", onBlur);
    window.addEventListener("resize", onBlur);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      window.removeEventListener("blur", onBlur);
      window.removeEventListener("resize", onBlur);
    };
  }, [onClose]);

  return (
    <div
      ref={ref}
      data-part="context-menu"
      data-geom={geom}
      data-key-context="menu"
      data-key-modal=""
      onContextMenu={(event) => event.preventDefault()}
    >
      <MenuList
        items={items}
        label={label}
        level={0}
        autoFocus={autoFocus}
        onRun={onRun}
        onClose={onClose}
      />
    </div>
  );
}
