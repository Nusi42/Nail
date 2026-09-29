import { useLabel } from "../i18n/I18nProvider";
import { optionString, type WidgetProps } from "../layout/widgetRegistry";

/** Static text from the `text` option (literal or `@message.id`). */
export function Text({ node }: WidgetProps) {
  const label = useLabel();
  const text = optionString(node, "text", node.label ?? "");
  return (
    <div data-part="text" data-node-id={node.id ?? undefined}>
      {text ? label(text) : null}
    </div>
  );
}
