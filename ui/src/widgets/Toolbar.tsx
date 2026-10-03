import type { KeyboardEvent } from "react";
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

/** Left / Right / Home / End move between a toolbar's enabled buttons. */
export function onToolbarKeyDown(event: KeyboardEvent<HTMLElement>): void {
  const keys = ["ArrowLeft", "ArrowRight", "Home", "End"];
  if (!keys.includes(event.key) || event.altKey || event.ctrlKey || event.metaKey) return;
  const buttons = [
    ...event.currentTarget.querySelectorAll<HTMLButtonElement>(
      '[data-part="toolbar.button"]:not(:disabled)',
    ),
  ];
  if (buttons.length === 0) return;
  const at = buttons.indexOf(event.target as HTMLButtonElement);
  let next: number;
  if (event.key === "Home") next = 0;
  else if (event.key === "End") next = buttons.length - 1;
  else if (at < 0) next = 0;
  else next = (at + (event.key === "ArrowRight" ? 1 : -1) + buttons.length) % buttons.length;
  event.preventDefault();
  buttons[next]?.focus();
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
      onKeyDown={onToolbarKeyDown}
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
          case "more":
            // The "More" menu (M5b, ADR 0109) is rendered by WP162.
            return null;
        }
      })}
    </div>
  );
}
