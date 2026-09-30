/**
 * Placeholder of a plug-in panel (`plugin-panel "<plugin>.<panel>"` in
 * `layout.kdl`; contract ADR 0050). WP62 replaces it with the renderer of
 * the panel's component tree (`GetPluginPanel`, `PluginPanelChanged`).
 */
import { useT } from "../../i18n/I18nProvider";
import { optionString, type WidgetProps } from "../../layout/widgetRegistry";

export function PluginPanel({ node }: WidgetProps) {
  const t = useT();
  const panel = optionString(node, "panel", "");
  return (
    <section
      data-part="plugin-panel"
      data-panel={panel}
      data-node-id={node.id ?? undefined}
      aria-label={t("plugin-panel-label")}
    >
      <p data-part="plugin-panel.missing">{t("plugin-panel-missing")}</p>
    </section>
  );
}
