/**
 * Mounted once in the app shell: opens the layout's context menu for the
 * part under a right-click (and for keyboard `contextmenu` events), renders
 * the open menu without the actions that do not apply to its target (M5b)
 * and runs its actions with the menu's target as context.
 *
 * Shift+F10 and the menu key are ordinary key bindings of the action
 * `menu.context` (default layout), so they can be rebound.
 */
import { useCallback, useEffect } from "react";
import type { JsonValue } from "../../api/generated";
import { useActions } from "../../actions/ActionsProvider";
import { useT } from "../../i18n/I18nProvider";
import { useLayoutConfig } from "../../layout/LayoutContext";
import { useUiStoreApi } from "../../state/uiStore";
import { ContextMenu } from "./ContextMenu";
import { withoutConversationShortcuts, withoutDisabled } from "./resolve";
import { menuStoreFor, openContextMenu, useMenu } from "./store";

export function ContextMenuHost() {
  const t = useT();
  const ui = useUiStoreApi();
  const layout = useLayoutConfig();
  const { dispatch, isEnabled } = useActions();
  const open = useMenu((s) => s.open);

  useEffect(() => {
    const onContextMenu = (event: MouseEvent) => {
      if (event.defaultPrevented) return;
      const element = event.target instanceof Element ? event.target : null;
      // A keyboard-invoked event has no pointer position.
      const keyboard = event.clientX === 0 && event.clientY === 0 && event.button !== 2;
      const point = keyboard ? null : { x: event.clientX, y: event.clientY };
      if (openContextMenu(ui, layout, element, { point })) event.preventDefault();
    };
    document.addEventListener("contextmenu", onContextMenu);
    return () => {
      document.removeEventListener("contextmenu", onContextMenu);
    };
  }, [ui, layout]);

  const close = useCallback(
    (restoreFocus: boolean) => {
      menuStoreFor(ui).getState().close(restoreFocus);
    },
    [ui],
  );

  const run = useCallback(
    (action: string, args: readonly JsonValue[]) => {
      // The handler reads the menu's target synchronously, before the close.
      const done = dispatch(action, args);
      close(true);
      void done;
    },
    [dispatch, close],
  );

  // Actions that do not apply to the target are hidden, not greyed out.
  const enabled = open ? withoutDisabled(open.menu.items, isEnabled) : [];
  // A card's actions act on its message, not on the conversation the keys reach.
  const items = open?.target.card ? withoutConversationShortcuts(enabled) : enabled;
  useEffect(() => {
    if (open && items.length === 0) close(true);
  }, [open, items.length, close]);

  if (!open || items.length === 0) return null;
  return (
    <ContextMenu
      key={`${open.menu.name}-${String(open.x)}-${String(open.y)}`}
      items={items}
      label={t(`context-menu-label-${open.target.part.replaceAll(".", "-")}`)}
      x={open.x}
      y={open.y}
      focusOnOpen
      onRun={run}
      onClose={close}
    />
  );
}
