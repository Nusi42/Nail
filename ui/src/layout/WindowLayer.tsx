/**
 * Secondary windows (composer, settings, account setup, outbox, move picker)
 * shown as modal dialogs over the main window. Their content is the layout's
 * `window "<name>"` tree, so it is as customizable as the main window. The
 * desktop shell may later open them as real windows.
 */
import { useEffect, useRef } from "react";
import type { ResolvedLayout } from "../api/generated";
import { useT } from "../i18n/I18nProvider";
import { useUi } from "../state/uiStore";
import { Icon } from "../widgets/Icon";
import { WindowView } from "./LayoutRenderer";
import { useDialogRegistry } from "./dialogRegistry";

const FOCUSABLE =
  'input:not([disabled]), textarea:not([disabled]), select:not([disabled]), button:not([disabled]), [tabindex="0"]';

/** Fluent id of a window's title: `window-<name>`. */
export function windowTitleId(name: string): string {
  return `window-${name}`;
}

function WindowDialog({
  layout,
  name,
  top,
}: {
  layout: ResolvedLayout;
  name: string;
  top: boolean;
}) {
  const t = useT();
  const closeWindow = useUi((s) => s.closeWindow);
  const dialogs = useDialogRegistry();
  const ref = useRef<HTMLDivElement>(null);
  // A layout window wins; a feature dialog fills in for a name the layout lacks.
  const Dialog = Object.hasOwn(layout.windows, name) ? undefined : dialogs[name];
  const titleId = `nail-window-${name}-title`;

  // Focus the first field when the window opens; give focus back on close.
  useEffect(() => {
    const previous = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    const el = ref.current;
    const body = el?.querySelector('[data-part="window.body"]');
    const first = body?.querySelector<HTMLElement>(FOCUSABLE) ?? el;
    first?.focus();
    return () => {
      if (previous?.isConnected) previous.focus();
    };
  }, []);

  return (
    <div
      ref={ref}
      data-part="window"
      data-window={name}
      data-state={top ? "active" : undefined}
      data-key-context="window"
      data-key-modal=""
      role="dialog"
      aria-modal={top}
      aria-labelledby={titleId}
      tabIndex={-1}
    >
      <div data-part="window.header">
        <h2 data-part="window.title" id={titleId}>
          {t(windowTitleId(name))}
        </h2>
        <button
          type="button"
          data-part="window.close"
          aria-label={t("window-close")}
          title={t("window-close")}
          onClick={() => closeWindow(name)}
        >
          <Icon name="close" part="window.close.icon" />
        </button>
      </div>
      <div data-part="window.body">
        {Dialog ? (
          <Dialog name={name} close={() => closeWindow(name)} />
        ) : (
          <WindowView layout={layout} name={name} />
        )}
      </div>
    </div>
  );
}

/** Renders every open secondary window, topmost last. */
export function WindowLayer({ layout }: { layout: ResolvedLayout }) {
  const windows = useUi((s) => s.windows);
  if (windows.length === 0) return null;
  return (
    <div data-part="window-layer">
      {windows.map((name, index) => (
        <WindowDialog key={name} layout={layout} name={name} top={index === windows.length - 1} />
      ))}
    </div>
  );
}
