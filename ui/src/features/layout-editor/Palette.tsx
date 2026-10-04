/**
 * The palette beside the window in customize mode (M5d WP181): the
 * window's presets (Three panes, Reader below, Wide reader, Focus for the
 * mail view; only those whose panes the breakpoint has can be applied) and
 * "Add to layout" — widgets and installed plug-ins' panels, dragged onto a
 * pane's drop target, or picked up with Enter / a click and dropped with
 * the keyboard like a pane. Toolbars are customized from their own context
 * menu (WP182); the palette says so.
 */
import { useId } from "react";
import type { LayoutPreset, PresetNode } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useLayoutConfig } from "../../layout/LayoutContext";
import { Icon } from "../../widgets/Icon";
import { usePlugins } from "../plugins/queries";
import { useDragSource } from "./dnd";
import {
  PALETTE_WIDGETS,
  newWidget,
  pluginPanelWidget,
  presetActive,
  presetFits,
  presetsOf,
  type Moving,
} from "./model";
import { usePaletteTexts, usePresetName } from "./names";
import { setMoving, useEditor } from "./store";
import { useLayoutEdit } from "./useLayoutEdit";

export interface PaletteProps {
  window: string;
  breakpoint: number;
}

export function Palette({ window, breakpoint }: PaletteProps) {
  const t = useT();
  const id = useId();
  const layout = useLayoutConfig();
  const presetName = usePresetName();
  const paletteTexts = usePaletteTexts();
  const edit = useLayoutEdit();
  const busy = useEditor((s) => s.busy);
  const plugins = usePlugins();
  const root = layout.windows[window]?.[breakpoint]?.root ?? null;
  const presets = presetsOf(layout, window);

  const panels = (plugins.data ?? [])
    .filter((plugin) => plugin.enabled)
    .flatMap((plugin) =>
      (plugin.manifest.contributes?.panels ?? []).map((panel) => ({
        key: `${plugin.id}.${panel.id}`,
        moving: {
          kind: "widget" as const,
          widget: pluginPanelWidget(plugin.id, panel.id),
          name: panel.title,
        },
        icon: panel.icon ?? "puzzle",
        name: panel.title,
        description: t("layout-editor-widget-plugin-panel-description", {
          plugin: plugin.manifest.name,
        }),
      })),
    );

  const builtins = PALETTE_WIDGETS.map((w) => {
    const texts = paletteTexts(w.kind);
    return {
      key: w.kind,
      moving: { kind: "widget" as const, widget: newWidget(w.kind, w.options), name: texts.name },
      icon: w.icon,
      name: texts.name,
      description: texts.description,
    };
  });

  const apply = (preset: LayoutPreset) => {
    void edit(
      [{ op: "apply-preset", preset: preset.id, target: { window, breakpoint, path: [] } }],
      { id: "layout-editor-preset-applied", args: { preset: presetName(preset) } },
    );
  };

  return (
    <aside data-part="layout-editor.palette" aria-label={t("layout-editor-palette")}>
      <h3 data-part="layout-editor.palette-title" id={`${id}-presets`}>
        {t("layout-editor-presets")}
      </h3>
      {presets.length === 0 ? (
        <p data-part="form.hint">{t("layout-editor-presets-none")}</p>
      ) : (
        <div data-part="layout-editor.presets" role="group" aria-labelledby={`${id}-presets`}>
          {presets.map((preset) => {
            const fits = root !== null && presetFits(preset, root);
            const active = fits && presetActive(preset, root);
            return (
              <button
                key={preset.id}
                type="button"
                data-part="layout-editor.preset"
                data-preset={preset.id}
                aria-pressed={active}
                aria-describedby={fits ? undefined : `${id}-missing`}
                disabled={busy || !fits}
                onClick={() => {
                  apply(preset);
                }}
              >
                <PresetPreview node={preset.root} />
                <span data-part="layout-editor.preset-name">{presetName(preset)}</span>
              </button>
            );
          })}
          {root !== null && presets.some((preset) => !presetFits(preset, root)) && (
            <p data-part="form.hint" id={`${id}-missing`}>
              {t("layout-editor-presets-missing")}
            </p>
          )}
        </div>
      )}
      <h3 data-part="layout-editor.palette-title" id={`${id}-add`}>
        {t("layout-editor-add")}
      </h3>
      <p data-part="form.hint">{t("layout-editor-add-hint")}</p>
      <ul data-part="layout-editor.widgets" aria-labelledby={`${id}-add`}>
        {[...builtins, ...panels].map((entry) => (
          <li key={entry.key} data-part="layout-editor.widgets.item">
            <PaletteWidget
              moving={entry.moving}
              icon={entry.icon}
              name={entry.name}
              description={entry.description}
              busy={busy}
            />
          </li>
        ))}
      </ul>
      <h3 data-part="layout-editor.palette-title">{t("layout-editor-toolbars")}</h3>
      <p data-part="form.hint">{t("layout-editor-toolbars-hint")}</p>
    </aside>
  );
}

function PaletteWidget({
  moving,
  icon,
  name,
  description,
  busy,
}: {
  moving: Extract<Moving, { kind: "widget" }>;
  icon: string;
  name: string;
  description: string;
  busy: boolean;
}) {
  const t = useT();
  const descriptionId = useId();
  const current = useEditor((s) => s.moving);
  const picked =
    current?.kind === "widget" &&
    current.widget.kind === moving.widget.kind &&
    JSON.stringify(current.widget.options) === JSON.stringify(moving.widget.options);
  const { onPointerDown, isDragClick } = useDragSource(() => moving);
  return (
    <button
      type="button"
      data-part="layout-editor.widget"
      data-kind={moving.widget.kind}
      aria-pressed={picked}
      aria-label={t(picked ? "layout-editor-put-back" : "layout-editor-pick-up", { pane: name })}
      aria-describedby={descriptionId}
      disabled={busy}
      onPointerDown={onPointerDown}
      onClick={() => {
        if (isDragClick()) return;
        setMoving(picked ? null : moving);
      }}
    >
      <span data-part="layout-editor.widget-icon">
        <Icon name={icon} part="layout-editor.widget-icon.svg" />
      </span>
      <span data-part="layout-editor.widget-text">
        <span data-part="layout-editor.widget-name">{name}</span>
        <span data-part="layout-editor.widget-description" id={descriptionId}>
          {description}
        </span>
      </span>
    </button>
  );
}

/** A small drawing of a preset's arrangement (structure only; the theme draws it). */
export function PresetPreview({ node }: { node: PresetNode }) {
  return (
    <span data-part="layout-editor.preset-preview" aria-hidden="true">
      <PreviewNode node={node} />
    </span>
  );
}

function PreviewNode({ node }: { node: PresetNode }) {
  if (node.type === "pane") {
    return <span data-part="layout-editor.preset-preview.pane" data-pane={node.id} />;
  }
  return (
    <span
      data-part="layout-editor.preset-preview.group"
      data-orientation={node.type === "split" ? node.orientation : "vertical"}
    >
      {node.children.map((child, index) => (
        <PreviewNode key={index} node={child} />
      ))}
    </span>
  );
}
