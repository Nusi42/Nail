/**
 * A pane in customize mode (M5d WP181): the real pane (not usable while
 * customizing: `inert`) under a header with the drag handle, its name, the
 * hide / show eye and Remove; while something is picked up or dragged,
 * the drop targets (dock left / right / above / below, swap, or add into).
 *
 * Keyboard: the handle picks the pane up (Enter / Space; again puts it
 * back), Tab reaches the targets of the other panes, Enter drops, Escape
 * cancels (`./EditorFrame.tsx`). The pointer drags the handle onto a
 * target; a click on the handle picks the pane up like the keyboard.
 */
import { useEffect, useId, useRef, type ReactNode } from "react";
import type { LayoutNode, LayoutNodeRef } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useLayoutConfig } from "../../layout/LayoutContext";
import { Icon } from "../../widgets/Icon";
import { TARGET_PART, useDragSource } from "./dnd";
import {
  dropPatch,
  isFixedRoot,
  isPane,
  nodeOf,
  refKey,
  rootOf,
  sameRef,
  zonesFor,
  type DropZone,
  type Moving,
} from "./model";
import { usePaneName, useZoneTexts } from "./names";
import { editorStore, setMoving, useEditor } from "./store";
import { useLayoutEdit } from "./useLayoutEdit";

/** Lucide icons of the drop zones. */
const ZONE_ICONS: Readonly<Record<DropZone, string>> = {
  left: "chevron-left",
  right: "chevron-right",
  top: "chevron-up",
  bottom: "chevron-down",
  swap: "arrow-left-right",
  into: "plus",
};

export interface PaneChromeProps {
  node: LayoutNode;
  nodeRef: LayoutNodeRef;
  children: ReactNode;
}

/** The chrome of one rendered node: a pane frame for panes, else the node itself. */
export function PaneChrome({ node, nodeRef, children }: PaneChromeProps) {
  const layout = useLayoutConfig();
  const root = rootOf(layout, nodeRef);
  if (!root) return children;
  const fixed = isFixedRoot(root, nodeRef.path);
  if (fixed || isPane(root, nodeRef.path)) {
    return (
      <PaneFrame node={node} nodeRef={nodeRef} fixed={fixed}>
        {children}
      </PaneFrame>
    );
  }
  // Hidden nodes inside a pane stay out of sight while customizing.
  return node.hidden === true ? null : children;
}

interface PaneFrameProps extends PaneChromeProps {
  /** The window is this one widget: it cannot move, hide or go. */
  fixed: boolean;
}

function PaneFrame({ node, nodeRef, fixed, children }: PaneFrameProps) {
  const t = useT();
  const titleId = useId();
  const paneName = usePaneName();
  const zoneTexts = useZoneTexts();
  const edit = useLayoutEdit();
  const moving = useEditor((s) => s.moving);
  const dragging = useEditor((s) => s.dragging);
  const busy = useEditor((s) => s.busy);
  const key = refKey(nodeRef);
  const hoverZone = useEditor((s) => (s.hover?.pane === key ? s.hover.zone : null));
  const focusPane = useEditor((s) => s.focusPane);
  const handleRef = useRef<HTMLButtonElement>(null);
  const name = paneName(node);
  const hidden = node.hidden === true;
  const picked = moving?.kind === "pane" && sameRef(moving.ref, nodeRef);
  const zones = moving && !picked ? zonesFor(moving, nodeRef, node) : [];
  const layout = useLayoutConfig();

  const describe = (): Moving => ({ kind: "pane", ref: nodeRef });
  const { onPointerDown, isDragClick } = useDragSource(describe);

  // After an edit, the focus comes back to the pane that moved.
  useEffect(() => {
    if (focusPane === null || node.id !== focusPane) return;
    editorStore.setState({ focusPane: null });
    handleRef.current?.focus();
  }, [focusPane, node.id]);

  const movingName = (m: Moving): string => {
    if (m.kind === "widget") return m.name;
    const target = nodeOf(layout, m.ref);
    return target ? paneName(target) : "";
  };

  const drop = (zone: DropZone) => {
    const current = editorStore.getState().moving;
    if (!current) return;
    const what = movingName(current);
    const focusId =
      current.kind === "pane" ? (nodeOf(layout, current.ref)?.id ?? null) : current.widget.id;
    void edit(
      [dropPatch(current, nodeRef, node, zone)],
      {
        id: `layout-editor-dropped-${zone}`,
        args: { moving: what, pane: name },
      },
      focusId,
    );
  };

  const state = [hidden && "hidden", picked && "picked", zones.length > 0 && "target"]
    .filter(Boolean)
    .join(" ");

  return (
    <section
      data-part="layout-editor.pane"
      data-pane={key}
      data-node-id={node.id ?? undefined}
      data-state={state === "" ? undefined : state}
      aria-labelledby={titleId}
    >
      <div data-part="layout-editor.pane-header">
        {fixed ? (
          <Icon name="lock" part="layout-editor.pane-lock" />
        ) : (
          <button
            ref={handleRef}
            type="button"
            data-part="layout-editor.handle"
            data-pane={key}
            aria-label={t(picked ? "layout-editor-put-back" : "layout-editor-pick-up", {
              pane: name,
            })}
            aria-pressed={picked}
            title={t("layout-editor-handle-hint")}
            disabled={busy}
            onPointerDown={onPointerDown}
            onClick={() => {
              if (isDragClick()) return;
              setMoving(picked ? null : { kind: "pane", ref: nodeRef });
            }}
          >
            <Icon name="grip-vertical" part="layout-editor.handle.icon" />
          </button>
        )}
        <h3 data-part="layout-editor.pane-title" id={titleId}>
          {name}
        </h3>
        {hidden && <span data-part="layout-editor.pane-badge">{t("layout-editor-hidden")}</span>}
        {!fixed && (
          <>
            <button
              type="button"
              data-part="form.button"
              data-variant="icon"
              data-action="toggle-hidden"
              aria-label={t(hidden ? "layout-editor-show" : "layout-editor-hide", { pane: name })}
              title={t(hidden ? "layout-editor-show" : "layout-editor-hide", { pane: name })}
              disabled={busy}
              onClick={() => {
                void edit(
                  [{ op: "set-hidden", node: nodeRef, hidden: !hidden }],
                  {
                    id: hidden ? "layout-editor-shown" : "layout-editor-hidden-done",
                    args: { pane: name },
                  },
                  node.id,
                );
              }}
            >
              <Icon name={hidden ? "eye" : "eye-off"} part="form.button.icon" />
            </button>
            <button
              type="button"
              data-part="form.button"
              data-variant="icon"
              data-action="remove"
              aria-label={t("layout-editor-remove", { pane: name })}
              title={t("layout-editor-remove", { pane: name })}
              disabled={busy}
              onClick={() => {
                void edit([{ op: "remove-node", node: nodeRef }], {
                  id: "layout-editor-removed",
                  args: { pane: name },
                });
              }}
            >
              <Icon name="trash" part="form.button.icon" />
            </button>
          </>
        )}
      </div>
      <div data-part="layout-editor.pane-body" inert>
        {children}
      </div>
      {moving && zones.length > 0 && (
        <div
          data-part="layout-editor.targets"
          data-dragging={dragging ? "" : undefined}
          role="group"
          aria-label={t("layout-editor-targets", { pane: name })}
        >
          {zones.map((zone) => {
            const texts = zoneTexts(zone, movingName(moving), name);
            return (
              <button
                key={zone}
                type="button"
                data-part={TARGET_PART}
                data-pane={key}
                data-zone={zone}
                data-state={hoverZone === zone ? "hover" : undefined}
                aria-label={texts.long}
                title={texts.long}
                disabled={busy}
                onClick={() => {
                  drop(zone);
                }}
              >
                <Icon name={ZONE_ICONS[zone]} part="layout-editor.target.icon" />
                <span data-part="layout-editor.target.label">{texts.short}</span>
              </button>
            );
          })}
        </div>
      )}
    </section>
  );
}
