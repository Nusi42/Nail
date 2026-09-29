/**
 * The on-screen indicator (layout widget `mcp-context-indicator`): shown
 * while an agent reads (or edits) what the user has on screen, i.e. for a
 * few seconds after each `McpContextAccess` event (design.md, "Permissions
 * and safety"). The status region is always mounted so screen readers
 * announce it.
 */
import { useLabel, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { Icon } from "../../widgets/Icon";
import { useMcpState } from "./state";

export function ContextIndicator({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const access = useMcpState((s) => s.access);
  const clear = useMcpState((s) => s.clear);
  return (
    <div
      data-part="mcp-context-indicator"
      data-node-id={node.id ?? undefined}
      data-state={access ? (access.edit ? "editing" : "reading") : "idle"}
      role="status"
      aria-live="polite"
      aria-label={node.label ? label(node.label) : t("mcp-indicator-label")}
    >
      {access && (
        <>
          <span data-part="mcp-context-indicator.dot" aria-hidden="true" />
          <span data-part="mcp-context-indicator.text">
            {t(access.edit ? "mcp-indicator-editing" : "mcp-indicator-reading", {
              client: access.clientName,
            })}
          </span>
          <button
            type="button"
            data-part="mcp-context-indicator.dismiss"
            aria-label={t("mcp-indicator-dismiss")}
            onClick={clear}
          >
            <Icon name="close" part="mcp-context-indicator.dismiss.icon" />
          </button>
        </>
      )}
    </div>
  );
}
