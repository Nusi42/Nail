/**
 * The confirmation policy of one MCP client or plug-in: per kind of write,
 * "ask first" or "run without asking". Deleting (Trash, permanent delete,
 * deleting events / contacts / tasks) always asks and cannot be changed.
 * Mounted by the MCP client settings and the plug-in manager (both settings
 * sections): nothing is written before the page's Save.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useId } from "react";
import { errorMessageOf } from "../../api/client";
import type { ActionActor, ActionKind, ActionPolicy, ActionPolicyMode } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useClient } from "../../state/queries";
import { useFormNotice, useSettingsForm } from "../settings/draft";
import { actorKey, defaultMode, isOutgoing, kindMessageId } from "./model";
import { agentsKeys, useActionPolicies } from "./queries";

interface Props {
  actor: ActionActor;
  /** The client's or plug-in's name. */
  name: string;
  /** No changes (a revoked client). */
  disabled?: boolean;
}

function modeMessageId(mode: ActionPolicyMode): string {
  return mode === "ask" ? "agents-policy-ask" : "agents-policy-allow";
}

function PolicyRow({
  policy,
  disabled,
  onChange,
}: {
  policy: ActionPolicy;
  disabled: boolean;
  onChange: (kind: ActionKind, mode: ActionPolicyMode | null) => void;
}) {
  const t = useT();
  const name = useId();
  const label = t(kindMessageId(policy.kind));
  return (
    <li
      data-part="action-policies.item"
      data-kind={policy.kind}
      data-mode={policy.mode}
      data-customized={policy.customized ? "" : undefined}
      data-locked={policy.can_allow ? undefined : ""}
      data-outgoing={isOutgoing(policy.kind) ? "" : undefined}
    >
      <fieldset data-part="action-policies.choice" disabled={disabled || !policy.can_allow}>
        <legend data-part="action-policies.kind">{label}</legend>
        {(["ask", "allow"] as const).map((mode) => (
          <label key={mode} data-part="action-policies.option">
            <input
              type="radio"
              name={`${name}-mode`}
              data-mode={mode}
              checked={policy.mode === mode}
              onChange={() => {
                onChange(policy.kind, mode === defaultMode(policy.kind) ? null : mode);
              }}
            />
            {t(modeMessageId(mode))}
            {mode === defaultMode(policy.kind) && (
              <span data-part="action-policies.default">{t("agents-policy-default")}</span>
            )}
          </label>
        ))}
      </fieldset>
      {!policy.can_allow && (
        <span data-part="action-policies.locked">{t("agents-policy-always-asks")}</span>
      )}
    </li>
  );
}

/** The kinds' modes as shown: the core's, or what the user picked before Save. */
type Modes = Partial<Record<ActionKind, ActionPolicyMode>>;

function modesOf(policies: readonly ActionPolicy[] | undefined): Modes {
  return Object.fromEntries((policies ?? []).map((p) => [p.kind, p.mode]));
}

/**
 * The policy table of `actor`. A settings draft (R6, ADR 0239): a changed
 * mode is written with the settings page's Save, one `SetActionPolicy` per
 * changed kind (the default mode removes the stored one); Discard drops it.
 */
export function PolicyEditor({ actor, name, disabled = false }: Props) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const policies = useActionPolicies(actor);
  const { notice, outcome, clear, problems } = useFormNotice();
  const form = useSettingsForm(modesOf(policies.data), {
    save: async (modes) => {
      const changed = (policies.data ?? []).filter(
        (p) => modes[p.kind] !== undefined && modes[p.kind] !== p.mode,
      );
      try {
        for (const policy of changed) {
          const mode = modes[policy.kind] ?? policy.mode;
          const answer = await client.request(
            {
              type: "SetActionPolicy",
              actor,
              kind: policy.kind,
              mode: mode === defaultMode(policy.kind) ? null : mode,
            },
            "ActionPolicies",
          );
          queryClient.setQueryData(agentsKeys.policies(actor), answer.policies);
        }
      } catch (error) {
        void queryClient.invalidateQueries({ queryKey: agentsKeys.policies(actor) });
        throw error;
      }
    },
    ...outcome("common-saved"),
  });
  const failure = problems([])[0];

  return (
    <section
      data-part="action-policies"
      data-actor={actorKey(actor)}
      aria-label={t("agents-policy-label", { name })}
    >
      <h4 data-part="action-policies.title">{t("agents-policy-title")}</h4>
      <p data-part="action-policies.intro">{t("agents-policy-intro", { name })}</p>
      {policies.isError && (
        <p data-part="action-policies.error" role="alert">
          {t(errorMessageOf(policies.error).id)}
        </p>
      )}
      {failure && (
        <p data-part="action-policies.error" role="alert">
          {t(failure.id, failure.args)}
        </p>
      )}
      {notice !== null && !form.dirty && (
        <p data-part="action-policies.status" role="status">
          {t(notice)}
        </p>
      )}
      {policies.data && (
        <ul data-part="action-policies.list">
          {policies.data.map((policy) => (
            <PolicyRow
              key={policy.kind}
              policy={{ ...policy, mode: form.value[policy.kind] ?? policy.mode }}
              disabled={disabled}
              onChange={(kind, mode) => {
                clear();
                form.set((modes) => ({ ...modes, [kind]: mode ?? defaultMode(kind) }));
              }}
            />
          ))}
        </ul>
      )}
    </section>
  );
}
