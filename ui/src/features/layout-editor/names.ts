/**
 * User-facing names of the layout editor (M5d WP181): panes (their layout
 * label, else the widget kind's name, else the node id), the windows it
 * customizes, presets and palette widgets. Texts in `layout-editor.ftl`.
 */
import { useCallback } from "react";
import type { LayoutNode, LayoutPreset, ResolvedLayout } from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import { viewLabel, viewOfWindow } from "../../layout/views";
import type { DropZone, PaletteKind } from "./model";

/** Widget kinds with a name of their own (`layout-editor-kind-<kind>`). */
export const NAMED_KINDS = [
  "folder-tree",
  "message-list",
  "message-view",
  "list-header",
  "thread-view",
  "toolbar",
  "mini-month",
  "agenda",
  "task-list",
  "smart-folders",
  "calendar",
  "calendar-list",
  "contact-list",
  "contact-card",
  "nav-rail",
  "view-host",
  "status-bar",
  "settings-page",
  "plugin-panel",
] as const;

export type NamedKind = (typeof NAMED_KINDS)[number];

function isNamedKind(kind: string): kind is NamedKind {
  return (NAMED_KINDS as readonly string[]).includes(kind);
}

/** The name of a pane. */
export function usePaneName(): (node: LayoutNode) => string {
  const t = useT();
  const label = useLabel();
  return useCallback(
    (node: LayoutNode) => {
      if (node.label !== null && node.label !== "") return label(node.label);
      if (node.type === "widget") {
        const panel = node.options.panel;
        if (node.kind === "plugin-panel" && typeof panel === "string") {
          return t("layout-editor-kind-plugin-panel-named", { panel });
        }
        if (isNamedKind(node.kind)) return t(`layout-editor-kind-${node.kind}`);
      }
      return t("layout-editor-pane-unnamed", {
        name: node.id ?? (node.type === "widget" ? node.kind : node.type),
      });
    },
    [t, label],
  );
}

/** The name of a window "Customize layout" offers. */
export function useWindowName(layout: ResolvedLayout): (window: string) => string {
  const t = useT();
  const label = useLabel();
  return useCallback(
    (window: string) => {
      const view = viewOfWindow(layout, window);
      if (view !== null) return label(viewLabel(layout, view));
      return window === "main"
        ? t("layout-editor-window-main")
        : t("layout-editor-window-other", { name: window });
    },
    [layout, label, t],
  );
}

/** The name of a preset. */
export function usePresetName(): (preset: LayoutPreset) => string {
  const label = useLabel();
  return useCallback(
    (preset: LayoutPreset) => label(preset.label ?? `@layout.preset.${preset.id}`),
    [label],
  );
}

/** Short text and accessible name of a drop zone. */
export function useZoneTexts(): (
  zone: DropZone,
  moving: string,
  pane: string,
) => { short: string; long: string } {
  const t = useT();
  return useCallback(
    (zone: DropZone, moving: string, pane: string) => ({
      short: t(`layout-editor-zone-${zone}`),
      long: t(`layout-editor-target-${zone}`, { moving, pane }),
    }),
    [t],
  );
}

/** Name and description of a built-in palette widget. */
export function usePaletteTexts(): (kind: PaletteKind) => { name: string; description: string } {
  const t = useT();
  return useCallback(
    (kind: PaletteKind) => ({
      name: t(`layout-editor-widget-${kind}`),
      description: t(`layout-editor-widget-${kind}-description`),
    }),
    [t],
  );
}
