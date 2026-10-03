/**
 * The navigation rail (layout widget `nav-rail`, M5b WP161, ADR 0109): the
 * app mark, one button per entry of the layout's `rail` section (views,
 * actions, spacers, separators) and the account avatar at the bottom.
 *
 * - A navigation landmark (`nav-rail-label`); the active view's button has
 *   `aria-current="page"`.
 * - Buttons show their icon; the label is visible with `show-labels=#true`
 *   and always the accessible name, with the badge's number when there is
 *   one ("Mail, 3 unread messages"). Tooltips name the shortcut.
 * - Up / Down / Home / End move between the buttons.
 * - Its own keyboard context (`global`): keys pressed on the rail never
 *   reach the hidden mail list.
 *
 * Options: `show-labels` (default `#false`), `show-logo` (default `#true`).
 */
import type { KeyboardEvent } from "react";
import type { BadgeSource, JsonValue, ViewDef } from "../../api/generated";
import { useActionEnabled, useActions } from "../../actions/ActionsProvider";
import { useLabel, useT } from "../../i18n/I18nProvider";
import { useLayoutConfig } from "../../layout/LayoutContext";
import { VIEW_SHOW_ACTION, currentView, viewLabel } from "../../layout/views";
import { optionBool, type WidgetProps } from "../../layout/widgetRegistry";
import { useAccounts } from "../../state/queries";
import { useUi } from "../../state/uiStore";
import { Icon } from "../../widgets/Icon";
import { Avatar } from "../../widgets/primitives";
import { ariaShortcut, keyLabel } from "../menus/keyLabels";
import { useBadgeCounts, type BadgeCounts } from "./badges";
import {
  actionShortcuts,
  badgeLabelId,
  badgeSources,
  badgeText,
  railEntries,
  viewShortcuts,
} from "./model";

const ITEM_SELECTOR = '[data-part="nav-rail.item"]:not(:disabled)';

/** Up / Down / Home / End move focus between the rail's enabled buttons. */
export function onRailKeyDown(event: KeyboardEvent<HTMLElement>): void {
  const keys = ["ArrowUp", "ArrowDown", "Home", "End"];
  if (!keys.includes(event.key) || event.altKey || event.ctrlKey || event.metaKey) return;
  const buttons = [...event.currentTarget.querySelectorAll<HTMLButtonElement>(ITEM_SELECTOR)];
  if (buttons.length === 0) return;
  const at = buttons.indexOf(event.target as HTMLButtonElement);
  let next: number;
  if (event.key === "Home") next = 0;
  else if (event.key === "End") next = buttons.length - 1;
  else if (at < 0) next = 0;
  else next = (at + (event.key === "ArrowDown" ? 1 : -1) + buttons.length) % buttons.length;
  event.preventDefault();
  buttons[next]?.focus();
}

interface ItemText {
  /** Accessible name (label + badge). */
  name: string;
  /** Tooltip (label + first shortcut). */
  title: string;
  shortcut: string | undefined;
}

/** Name, tooltip and shortcut of a rail button. */
function useItemText(
  text: string,
  shortcuts: readonly string[],
  badge: BadgeSource | null,
  counts: BadgeCounts,
): ItemText {
  const t = useT();
  const count = badge === null ? 0 : counts[badge];
  const name = badge !== null && count > 0 ? t(badgeLabelId(badge), { label: text, count }) : text;
  const shortcut = shortcuts[0];
  const title =
    shortcut === undefined
      ? name
      : t("shell-rail-tooltip", { label: name, shortcut: keyLabel(shortcut, t) });
  return { name, title, shortcut };
}

function Badge({ source, counts }: { source: BadgeSource | null; counts: BadgeCounts }) {
  const t = useT();
  if (source === null) return null;
  const count = counts[source];
  if (count <= 0) return null;
  const { id, args } = badgeText(count);
  return (
    <span data-part="nav-rail.badge" data-badge={source} aria-hidden="true">
      {t(id, args)}
    </span>
  );
}

function ViewItem({
  id,
  def,
  active,
  counts,
}: {
  id: string;
  def: ViewDef;
  active: boolean;
  counts: BadgeCounts;
}) {
  const label = useLabel();
  const layout = useLayoutConfig();
  const { dispatch } = useActions();
  const text = label(viewLabel(layout, id));
  const { name, title, shortcut } = useItemText(text, viewShortcuts(layout, id), def.badge, counts);
  return (
    <button
      type="button"
      data-part="nav-rail.item"
      data-view={id}
      data-state={active ? "active" : undefined}
      aria-current={active ? "page" : undefined}
      aria-label={name}
      aria-keyshortcuts={shortcut === undefined ? undefined : ariaShortcut(shortcut)}
      title={title}
      onClick={() => void dispatch(VIEW_SHOW_ACTION, [id])}
    >
      <Icon name={def.icon} part="nav-rail.icon" />
      <span data-part="nav-rail.label">{text}</span>
      <Badge source={def.badge} counts={counts} />
    </button>
  );
}

function ActionItem({
  action,
  args,
  labelOverride,
  iconOverride,
  badge,
  counts,
}: {
  action: string;
  args: JsonValue[];
  labelOverride: string | null;
  iconOverride: string | null;
  badge: BadgeSource | null;
  counts: BadgeCounts;
}) {
  const label = useLabel();
  const layout = useLayoutConfig();
  const { dispatch } = useActions();
  const enabled = useActionEnabled(action, args);
  const def = Object.hasOwn(layout.actions, action) ? layout.actions[action] : undefined;
  const text = label(labelOverride ?? def?.label ?? action);
  const { name, title, shortcut } = useItemText(
    text,
    actionShortcuts(layout, action, args),
    badge,
    counts,
  );
  return (
    <button
      type="button"
      data-part="nav-rail.item"
      data-action={action}
      data-state={enabled ? undefined : "disabled"}
      disabled={!enabled}
      aria-label={name}
      aria-keyshortcuts={shortcut === undefined ? undefined : ariaShortcut(shortcut)}
      title={title}
      onClick={() => void dispatch(action, args)}
    >
      <Icon name={iconOverride ?? def?.icon ?? null} part="nav-rail.icon" />
      <span data-part="nav-rail.label">{text}</span>
      <Badge source={badge} counts={counts} />
    </button>
  );
}

/**
 * The account at the bottom of the rail: the selected account, or the
 * first real one while "All accounts" (or a virtual account) is selected.
 */
function RailAccount() {
  const t = useT();
  const accounts = useAccounts();
  const activeId = useUi((s) => s.activeAccountId);
  const real = (accounts.data ?? []).filter((a) => a.kind !== "virtual");
  const account = real.find((a) => a.id === activeId) ?? real[0];
  if (account === undefined) return null;
  const name = t("shell-rail-account", { name: account.name || account.email });
  return (
    <span data-part="nav-rail.account" role="img" aria-label={name} title={name}>
      <Avatar name={account.name} email={account.email} />
    </span>
  );
}

/** Widget `nav-rail`. */
export function NavRail({ node }: WidgetProps) {
  const t = useT();
  const layout = useLayoutConfig();
  const active = useUi((s) => currentView(layout, s.activeView));
  const entries = railEntries(layout);
  const counts = useBadgeCounts(badgeSources(entries));
  const showLabels = optionBool(node, "show-labels", false);
  return (
    <nav
      data-part="nav-rail"
      data-node-id={node.id ?? undefined}
      data-labels={showLabels ? "shown" : "hidden"}
      data-key-context="global"
      aria-label={t("nav-rail-label")}
      onKeyDown={onRailKeyDown}
    >
      {optionBool(node, "show-logo", true) && (
        <span data-part="nav-rail.logo" aria-hidden="true">
          {t("shell-rail-logo")}
        </span>
      )}
      {entries.map((entry, i) => {
        switch (entry.type) {
          case "view":
            return (
              <ViewItem
                key={`view:${entry.id}`}
                id={entry.id}
                def={entry.def}
                active={entry.id === active}
                counts={counts}
              />
            );
          case "action":
            return (
              <ActionItem
                key={`action:${i}`}
                action={entry.action}
                args={entry.args}
                labelOverride={entry.label}
                iconOverride={entry.icon}
                badge={entry.badge}
                counts={counts}
              />
            );
          case "spacer":
            return <span key={`spacer:${i}`} data-part="nav-rail.spacer" aria-hidden="true" />;
          case "separator":
            return <span key={`separator:${i}`} data-part="nav-rail.separator" role="separator" />;
        }
      })}
      <RailAccount />
    </nav>
  );
}
