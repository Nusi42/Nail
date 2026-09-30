/**
 * Mounted once in the app shell: opens the layout's context menu for the
 * part under a right-click (and for keyboard `contextmenu` events), renders
 * the open menu and runs its actions with the menu's target as context.
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
import { menuStoreFor, openContextMenu, useMenu } from "./store";

export function ContextMenuHost() {
  const t = useT();
  const ui = useUiStoreApi();
  const layout = useLayoutConfig();
  const { dispatch } = useActions();
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
    return () => document.removeEventListener("contextmenu", onContextMenu);
  }, [ui, layout]);

  const close = useCallback(
    (restoreFocus: boolean) => menuStoreFor(ui).getState().close(restoreFocus),
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

  if (!open) return null;
  return (
    <ContextMenu
      key={`${open.menu.name}-${open.x}-${open.y}`}
      items={open.menu.items}
      label={t(`context-menu-label-${open.target.part.replaceAll(".", "-")}`)}
      x={open.x}
      y={open.y}
      autoFocus
      onRun={run}
      onClose={close}
    />
  );
}
