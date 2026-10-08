/**
 * Customize-layout mode around one window (M5d WP181, the proposal's
 * "Layout editor"): the banner ("Customizing layout", what to do, Undo,
 * Redo, Reset to default, Done), the window with pane headers, and the
 * palette (presets, "Add to layout", toolbars). Status messages are
 * announced politely; a conflict of Undo / Redo (the file changed on disk)
 * offers "Undo anyway".
 *
 * Keys inside the editor: Escape puts back what is picked up, else ends
 * customize mode (Done). The app's single-key shortcuts do not apply while
 * the focus is in the editor (`data-key-modal`), so a key never acts on
 * the mail behind it.
 */
import {
  useEffect,
  useEffectEvent,
  useId,
  useRef,
  type KeyboardEvent,
  type ReactNode,
} from "react";
import { useT } from "../../i18n/I18nProvider";
import { useLayoutConfig } from "../../layout/LayoutContext";
import {
  historyKey,
  isTextField,
  nextRedo,
  nextUndo,
  useEditHistory,
} from "../../state/editHistory";
import { Icon } from "../../widgets/Icon";
import { TARGET_PART } from "./dnd";
import { Palette } from "./Palette";
import { useWindowName } from "./names";
import { setMessage, setMoving, stopEditing, useEditor } from "./store";
import { useLayoutEdit, useLayoutStep } from "./useLayoutEdit";

export interface EditorFrameProps {
  window: string;
  breakpoint: number;
  children: ReactNode;
}

export function EditorFrame({ window, breakpoint, children }: EditorFrameProps) {
  const t = useT();
  const id = useId();
  const layout = useLayoutConfig();
  const windowName = useWindowName(layout);
  const history = useEditHistory("layout");
  const step = useLayoutStep();
  const edit = useLayoutEdit();
  const busy = useEditor((s) => s.busy);
  const moving = useEditor((s) => s.moving);
  const dragging = useEditor((s) => s.dragging);
  const message = useEditor((s) => s.message);
  const rootRef = useRef<HTMLDivElement>(null);
  const titleRef = useRef<HTMLHeadingElement>(null);
  /** What had the focus when something was picked up (focused again on Escape). */
  const pickedFrom = useRef<HTMLElement | null>(null);
  const undo = nextUndo(history.data);
  const redo = nextRedo(history.data);

  // Customize mode starts with the focus on its title.
  useEffect(() => {
    titleRef.current?.focus();
  }, []);

  // An edit removed the focused control (a drop target, a removed pane):
  // the focus goes to the title instead of the page.
  useEffect(() => {
    if (busy) return;
    const active = document.activeElement;
    if (active === null || active === document.body) titleRef.current?.focus();
  }, [busy, layout]);

  // Picked up with the keyboard (or a click): focus the first drop target.
  const onPicked = useEffectEvent(() => {
    const active = document.activeElement;
    pickedFrom.current = active instanceof HTMLElement ? active : null;
    rootRef.current?.querySelector<HTMLElement>(`[data-part="${TARGET_PART}"]`)?.focus();
  });
  useEffect(() => {
    if (moving !== null && !dragging) onPicked();
  }, [moving, dragging]);

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    // Ctrl+Z / Ctrl+Y: Undo / Redo (ADR 0138).
    const direction = event.defaultPrevented ? null : historyKey(event);
    if (direction !== null && !isTextField(event.target)) {
      event.preventDefault();
      if (!busy && (direction === "undo" ? undo : redo) !== null) step(direction);
      return;
    }
    if (event.key !== "Escape" || event.defaultPrevented) return;
    event.preventDefault();
    event.stopPropagation();
    if (moving !== null) {
      setMoving(null);
      setMessage({ kind: "status", text: { id: "layout-editor-cancelled" } });
      const back = pickedFrom.current;
      pickedFrom.current = null;
      if (back?.isConnected) back.focus();
      else titleRef.current?.focus();
    } else {
      stopEditing();
    }
  };

  const reset = () => {
    void edit([{ op: "reset-window", window }], {
      id: "layout-editor-reset-done",
      args: { window: windowName(window) },
    });
  };

  return (
    // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- Escape anywhere in the editor region cancels a pick-up or ends customize mode.
    <div
      ref={rootRef}
      data-part="layout-editor.frame"
      data-window={window}
      data-dragging={dragging ? "" : undefined}
      data-key-modal=""
      role="region"
      aria-labelledby={`${id}-title`}
      aria-busy={busy}
      onKeyDown={onKeyDown}
    >
      <div data-part="layout-editor.banner">
        <Icon name="layout-dashboard" part="layout-editor.banner-icon" />
        <h2 ref={titleRef} data-part="layout-editor.title" id={`${id}-title`} tabIndex={-1}>
          {t("layout-editor-title", { window: windowName(window) })}
        </h2>
        <span data-part="layout-editor.hint">{t("layout-editor-hint")}</span>
        <span data-part="layout-editor.banner-actions">
          <button
            type="button"
            data-part="form.button"
            data-variant="ghost"
            data-action="undo"
            disabled={busy || undo === null}
            title={
              undo
                ? t("layout-editor-undo-entry", { entry: t(undo.label.id, undo.label.args) })
                : undefined
            }
            onClick={() => {
              step("undo");
            }}
          >
            <Icon name="undo-2" part="form.button.icon" />
            {t("common-undo")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-variant="ghost"
            data-action="redo"
            disabled={busy || redo === null}
            title={
              redo
                ? t("layout-editor-redo-entry", { entry: t(redo.label.id, redo.label.args) })
                : undefined
            }
            onClick={() => {
              step("redo");
            }}
          >
            <Icon name="redo-2" part="form.button.icon" />
            {t("common-redo")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-variant="ghost"
            data-action="reset"
            disabled={busy}
            onClick={reset}
          >
            <Icon name="rotate-ccw" part="form.button.icon" />
            {t("layout-editor-reset")}
          </button>
          <button
            type="button"
            data-part="form.button"
            data-variant="primary"
            data-action="done"
            onClick={stopEditing}
          >
            <Icon name="check" part="form.button.icon" />
            {t("common-done")}
          </button>
        </span>
      </div>
      <div data-part="layout-editor.body">
        <div data-part="layout-editor.canvas">{children}</div>
        <Palette window={window} breakpoint={breakpoint} />
      </div>
      <div data-part="layout-editor.footer">
        <p
          data-part="layout-editor.status"
          data-state={message?.kind === "error" ? "error" : undefined}
          role="status"
        >
          {moving !== null && message === null
            ? t("layout-editor-moving")
            : message
              ? t(message.text.id, message.text.args)
              : null}
        </p>
        {message?.forceStep && (
          <button
            type="button"
            data-part="form.button"
            data-variant="danger"
            data-action="force"
            disabled={busy}
            onClick={() => {
              const direction = message.forceStep;
              if (direction) step(direction, true);
            }}
          >
            {t(
              message.forceStep === "undo"
                ? "layout-editor-undo-anyway"
                : "layout-editor-redo-anyway",
            )}
          </button>
        )}
      </div>
    </div>
  );
}
