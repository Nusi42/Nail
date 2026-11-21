/**
 * "+ Add condition" / "+ Add action" (R2 design review, item 9): a menu
 * button listing the kinds; picking one adds a blank one of that kind. It is
 * the app's menu button (`features/pim/PimMenu`, the WAI-ARIA menu button
 * pattern with the context menus' look), so the keys are the same as in
 * every other menu.
 */
import { useT } from "../../i18n/I18nProvider";
import { PimMenu, type PimMenuEntry } from "../pim/PimMenu";

export interface AddMenuProps<K extends string> {
  /** Fluent id of the button's (and the menu's) name. */
  labelId: string;
  /** `data-field` of the button. */
  field: string;
  /** The kinds, in menu order; `groups` (if any) follow after a separator. */
  kinds: readonly K[];
  groups?: readonly K[];
  /** A kind's label, localized. */
  labelOf: (kind: K) => string;
  onAdd: (kind: K) => void;
}

export function AddMenu<K extends string>({
  labelId,
  field,
  kinds,
  groups = [],
  labelOf,
  onAdd,
}: AddMenuProps<K>) {
  const t = useT();
  const item = (kind: K): PimMenuEntry => ({
    type: "item",
    id: kind,
    label: labelOf(kind),
    onSelect: () => {
      onAdd(kind);
    },
  });
  const entries: PimMenuEntry[] = [
    ...kinds.filter((k) => !groups.includes(k)).map(item),
    ...(groups.length > 0 ? [{ type: "separator", id: "groups" } as const] : []),
    ...groups.map(item),
  ];
  return (
    <span data-part="rules.add" data-field={field}>
      <PimMenu
        label={t(labelId)}
        icon="plus"
        entries={entries}
        field={field}
        part="rules.add.button"
        showLabel
      />
    </span>
  );
}
