import type { WidgetProps } from "../layout/widgetRegistry";

/** Takes up free space in a stack or split. */
export function Spacer({ node }: WidgetProps) {
  return <div data-part="spacer" data-node-id={node.id ?? undefined} aria-hidden="true" />;
}
