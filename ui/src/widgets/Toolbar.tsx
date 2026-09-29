import type { JsonValue } from "../api/generated";
import { useActionEnabled, useActions } from "../actions/ActionsProvider";
import { useLabel, useT } from "../i18n/I18nProvider";
import { useLayoutConfig } from "../layout/LayoutContext";
import { optionString, type WidgetProps } from "../layout/widgetRegistry";
import { Icon } from "./Icon";

interface ButtonProps {
  action: string;
  args: JsonValue[];
  label: string | null;
  icon: string | null;
}

function ToolbarButton({ action, args, label: labelOverride, icon: iconOverride }: ButtonProps) {
  const label = useLabel();
  const layout = useLayoutConfig();
  const { dispatch } = useActions();
  const enabled = useActionEnabled(action, args);
  const def = Object.hasOwn(layout.actions, action) ? layout.actions[action] : undefined;
  const text = label(labelOverride ?? def?.label ?? action);
  const icon = iconOverride ?? def?.icon ?? null;
  return (
    <button
      type="button"
      data-part="toolbar.button"
      data-action={action}
      data-state={enabled ? undefined : "disabled"}
      disabled={!enabled}
      title={text}
      onClick={() => void dispatch(action, args)}
    >
      <Icon name={icon} part="toolbar.button.icon" />
      <span data-part="toolbar.button.label">{text}</span>
    </button>
  );
}

/** A toolbar from the layout's `toolbars` section (`toolbar` option). */
export function Toolbar({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const layout = useLayoutConfig();
  const name = optionString(node, "toolbar", "");
  const items = Object.hasOwn(layout.toolbars, name) ? (layout.toolbars[name] ?? []) : [];
  return (
    <div
      data-part="toolbar"
      data-toolbar={name}
      data-node-id={node.id ?? undefined}
      role="toolbar"
      aria-label={node.label ? label(node.label) : t("toolbar-label")}
    >
      {items.map((item, index) => {
        switch (item.type) {
          case "action":
            return (
              <ToolbarButton
                key={index}
                action={item.action}
                args={item.args}
                label={item.label}
                icon={item.icon}
              />
            );
          case "spacer":
            return <span key={index} data-part="toolbar.spacer" />;
          case "separator":
            return <span key={index} data-part="toolbar.separator" role="separator" />;
        }
      })}
    </div>
  );
}
