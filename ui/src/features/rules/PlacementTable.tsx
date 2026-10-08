/**
 * Where each filter rule runs on a server (Sieve, Exchange inbox rules) and
 * why: one row per rule with its name, its placement and the reasons. The
 * feature that shows it names its parts and texts.
 */
import type { LocalizedMessage, RulePlacement } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useRules } from "./api";

/** One rule's placement as the core reports it. */
export interface PlacedRule {
  rule_id: string;
  placement: RulePlacement;
  reasons: LocalizedMessage[];
}

export interface PlacementTableProps {
  rules: readonly PlacedRule[];
  /** Parts: the table, its caption and head row, a rule row (`.name`, `.placement`, `.reasons`), the empty text. */
  parts: { table: string; caption: string; head: string; row: string; empty: string };
  /** Fluent ids: caption, the placement column, the empty text. */
  ids: { caption: string; placement: string; empty: string };
  /** A placement's text (the feature's own wording). */
  placementLabel: (placement: RulePlacement) => string;
}

export function PlacementTable({ rules, parts, ids, placementLabel }: PlacementTableProps) {
  const t = useT();
  const all = useRules();
  const names = new Map((all.data ?? []).map((r) => [r.id, r.name]));
  if (rules.length === 0) return <p data-part={parts.empty}>{t(ids.empty)}</p>;
  return (
    <table data-part={parts.table}>
      <caption data-part={parts.caption}>{t(ids.caption)}</caption>
      <thead>
        <tr data-part={parts.head}>
          <th scope="col">{t("common-rule")}</th>
          <th scope="col">{t(ids.placement)}</th>
          <th scope="col">{t("common-why")}</th>
        </tr>
      </thead>
      <tbody>
        {rules.map((rule) => (
          <tr key={rule.rule_id} data-part={parts.row} data-placement={rule.placement}>
            <th scope="row" data-part={`${parts.row}.name`}>
              {names.get(rule.rule_id) ?? rule.rule_id}
            </th>
            <td data-part={`${parts.row}.placement`}>{placementLabel(rule.placement)}</td>
            <td data-part={`${parts.row}.reasons`}>
              {rule.reasons.map((r) => t(r.id, r.args)).join(" ")}
            </td>
          </tr>
        ))}
      </tbody>
    </table>
  );
}
