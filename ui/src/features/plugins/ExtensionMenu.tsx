/**
 * A menu button listing plug-in extensions (composer actions, Markdown
 * blocks, attachment and link actions). Every item names its plug-in.
 * Keyboard: Enter / Space / ArrowDown open it and focus the first item,
 * arrows move, Home / End jump, Escape closes and returns focus, Tab
 * closes. Nothing is rendered when there is nothing to offer.
 */
import { useId, useRef, useState, type KeyboardEvent } from "react";
import { useT } from "../../i18n/I18nProvider";
import { Icon } from "../../widgets/Icon";
import type { Offer } from "./extensions";

interface Props {
  /** `data-menu` of the parts (which menu this is). */
  kind: string;
  /** Fluent id of the button's label. */
  labelId: string;
  labelArgs?: Record<string, string | number>;
  items: readonly Offer[];
  disabled?: boolean;
  onPick(item: Offer): void;
}

/** A labelled menu of plug-in extensions. */
export function ExtensionMenu({ kind, labelId, labelArgs, items, disabled, onPick }: Props) {
  const t = useT();
  const id = useId();
  const [open, setOpen] = useState(false);
  const button = useRef<HTMLButtonElement>(null);
  const menu = useRef<HTMLUListElement>(null);
  /** Which item to focus once the menu is shown. */
  const pending = useRef<"first" | "last" | null>(null);

  const focusItem = (list: HTMLUListElement | null, which: "first" | "last") => {
    const entries = list?.querySelectorAll<HTMLElement>('[role="menuitem"]');
    (which === "first" ? entries?.[0] : entries?.[entries.length - 1])?.focus();
  };

  const menuRef = (list: HTMLUListElement | null) => {
    menu.current = list;
    if (list && pending.current) {
      focusItem(list, pending.current);
      pending.current = null;
    }
  };

  const show = (which: "first" | "last") => {
    if (open) focusItem(menu.current, which);
    else pending.current = which;
    setOpen(true);
  };

  if (items.length === 0) return null;

  const close = (refocus: boolean) => {
    setOpen(false);
    if (refocus) button.current?.focus();
  };

  const onButtonKey = (event: KeyboardEvent<HTMLButtonElement>) => {
    if (event.key === "ArrowDown" || event.key === "ArrowUp") {
      event.preventDefault();
      show(event.key === "ArrowDown" ? "first" : "last");
    }
  };

  const onMenuKey = (event: KeyboardEvent<HTMLUListElement>) => {
    const entries = [...(menu.current?.querySelectorAll<HTMLElement>('[role="menuitem"]') ?? [])];
    const index = entries.indexOf(document.activeElement as HTMLElement);
    const move = (to: number) => {
      event.preventDefault();
      entries[(to + entries.length) % entries.length]?.focus();
    };
    switch (event.key) {
      case "ArrowDown":
        return move(index + 1);
      case "ArrowUp":
        return move(index - 1);
      case "Home":
        return move(0);
      case "End":
        return move(entries.length - 1);
      case "Escape":
        event.preventDefault();
        event.stopPropagation();
        return close(true);
      case "Tab":
        return close(false);
    }
  };

  return (
    <div data-part="plugin-extension-menu" data-menu={kind}>
      <button
        ref={button}
        type="button"
        data-part="plugin-extension-menu.button"
        data-menu={kind}
        aria-haspopup="menu"
        aria-expanded={open}
        aria-controls={open ? `${id}-menu` : undefined}
        disabled={disabled}
        onClick={() => (open ? setOpen(false) : show("first"))}
        onKeyDown={onButtonKey}
      >
        {t(labelId, labelArgs)}
      </button>
      {open && (
        <ul
          ref={menuRef}
          id={`${id}-menu`}
          role="menu"
          data-part="plugin-extension-menu.list"
          data-menu={kind}
          aria-label={t(labelId, labelArgs)}
          onKeyDown={onMenuKey}
          onBlur={(event) => {
            const next = event.relatedTarget as Node | null;
            if (next && !event.currentTarget.contains(next) && next !== button.current) {
              setOpen(false);
            }
          }}
        >
          {items.map((item) => (
            <li key={item.id} role="none" data-part="plugin-extension-menu.entry">
              <button
                type="button"
                role="menuitem"
                tabIndex={-1}
                data-part="plugin-extension-menu.item"
                data-plugin={item.pluginId}
                data-extension={item.id}
                title={item.description ?? undefined}
                onClick={() => {
                  close(true);
                  onPick(item);
                }}
              >
                <Icon name={item.icon} part="plugin-extension-menu.icon" />
                <span data-part="plugin-extension-menu.title">{item.title}</span>
                <span data-part="plugin-extension-menu.plugin">
                  {t("plugin-extension-from", { plugin: item.pluginName })}
                </span>
              </button>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}
