import { useQueryClient } from "@tanstack/react-query";
import { useState, type FormEvent } from "react";
import { errorMessageIdOf } from "../api/client";
import type {
  AccountConfig,
  FolderMapping,
  FolderSource,
  Identity,
  IdentityRule,
  VirtualAccountConfig,
} from "../api/generated";
import { useLabel, useT } from "../i18n/I18nProvider";
import { type WidgetProps } from "../layout/widgetRegistry";
import {
  queryKeys,
  useAccountsConfig,
  useClient,
  useFolders,
  useVirtualAccounts,
} from "../state/queries";
import { NoticeView, type Notice } from "./AccountSettings";
import { CheckField, Section, SelectField, TextField, type Choice } from "./forms";

/** A new virtual account over all real accounts. */
export function newVirtualAccount(members: readonly string[]): VirtualAccountConfig {
  return {
    id: "",
    name: "",
    members: [...members],
    merge_roles: true,
    folder_mappings: [],
    default_identity: null,
    identity_rules: [],
  };
}

/** Problems that keep a virtual account from being saved (Fluent ids). */
export function virtualProblems(config: VirtualAccountConfig): string[] {
  const problems: string[] = [];
  if (config.name.trim() === "") problems.push("virtual-error-name");
  if (config.members.length === 0) problems.push("virtual-error-members");
  if (config.folder_mappings.some((m) => m.name.trim() === "" || m.sources.length === 0)) {
    problems.push("virtual-error-mapping");
  }
  if (config.identity_rules.some((r) => !r.pattern.includes("@") || r.identity === "")) {
    problems.push("virtual-error-rule");
  }
  return problems;
}

/** Drops mapping sources, rules and the default identity that left with a member. */
export function pruneToMembers(
  config: VirtualAccountConfig,
  identities: readonly Identity[],
): VirtualAccountConfig {
  const members = new Set(config.members);
  const allowed = new Set(identities.filter((i) => members.has(i.account_id)).map((i) => i.id));
  return {
    ...config,
    folder_mappings: config.folder_mappings.map((m) => ({
      ...m,
      sources: m.sources.filter((s) => members.has(s.account_id)),
    })),
    default_identity:
      config.default_identity !== null && allowed.has(config.default_identity)
        ? config.default_identity
        : null,
    identity_rules: config.identity_rules.filter((r) => allowed.has(r.identity)),
  };
}

function SourceRow({
  source,
  accounts,
  index,
  onChange,
  onRemove,
}: {
  source: FolderSource;
  accounts: readonly AccountConfig[];
  index: string;
  onChange: (source: FolderSource) => void;
  onRemove: () => void;
}) {
  const t = useT();
  const folders = useFolders(source.account_id || null);
  const folderChoices: Choice[] = (folders.data ?? []).map((f) => ({
    value: f.path,
    label: f.path,
    literal: true,
  }));
  return (
    <li data-part="virtual-editor.source">
      <SelectField
        labelId="virtual-source-account"
        name={`source-${index}-account`}
        value={source.account_id}
        choices={accounts.map((a) => ({ value: a.id, label: a.name, literal: true }))}
        onChange={(account_id) => onChange({ account_id, path: "" })}
      />
      <SelectField
        labelId="virtual-source-folder"
        name={`source-${index}-folder`}
        value={source.path}
        choices={[{ value: "", label: "virtual-source-choose" }, ...folderChoices]}
        onChange={(path) => onChange({ ...source, path })}
      />
      <button type="button" data-part="form.button" data-action="remove-source" onClick={onRemove}>
        {t("virtual-remove-source")}
      </button>
    </li>
  );
}

function MappingEditor({
  mapping,
  index,
  accounts,
  onChange,
  onRemove,
}: {
  mapping: FolderMapping;
  index: number;
  accounts: readonly AccountConfig[];
  onChange: (mapping: FolderMapping) => void;
  onRemove: () => void;
}) {
  const t = useT();
  const first = accounts[0];
  return (
    <li data-part="virtual-editor.mapping" data-index={index}>
      <TextField
        labelId="virtual-mapping-name"
        name={`mapping-${index}-name`}
        value={mapping.name}
        onChange={(name) => onChange({ ...mapping, name })}
      />
      <ul data-part="virtual-editor.sources">
        {mapping.sources.map((source, i) => (
          <SourceRow
            key={i}
            index={`${index}-${i}`}
            source={source}
            accounts={accounts}
            onChange={(next) =>
              onChange({
                ...mapping,
                sources: mapping.sources.map((old, j) => (j === i ? next : old)),
              })
            }
            onRemove={() =>
              onChange({ ...mapping, sources: mapping.sources.filter((_, j) => j !== i) })
            }
          />
        ))}
      </ul>
      <button
        type="button"
        data-part="form.button"
        data-action="add-source"
        disabled={!first}
        onClick={() =>
          onChange({
            ...mapping,
            sources: [...mapping.sources, { account_id: first?.id ?? "", path: "" }],
          })
        }
      >
        {t("virtual-add-source")}
      </button>
      <button type="button" data-part="form.button" data-action="remove-mapping" onClick={onRemove}>
        {t("virtual-remove-mapping")}
      </button>
    </li>
  );
}

function VirtualForm({
  initial,
  accounts,
  onDone,
  setMessage,
}: {
  initial: VirtualAccountConfig;
  accounts: readonly AccountConfig[];
  onDone: (id: string | null) => void;
  setMessage: (notice: Notice) => void;
}) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const [config, setConfig] = useState(initial);
  const [busy, setBusy] = useState(false);
  const isNew = initial.id === "";
  const identities = accounts.flatMap((a) => a.identities);
  const memberAccounts = accounts.filter((a) => config.members.includes(a.id));
  const memberIdentities = identities.filter((i) => config.members.includes(i.account_id));
  const identityChoices: Choice[] = memberIdentities.map((i) => ({
    value: i.id,
    label: i.name ? `${i.name} <${i.email}>` : i.email,
    literal: true,
  }));
  // Clean mappings and rules only when saving (valid sources must not vanish while editing).
  const toSave = pruneToMembers(
    {
      ...config,
      folder_mappings: config.folder_mappings.map((m) => ({
        ...m,
        sources: m.sources.filter((s) => s.path !== ""),
      })),
    },
    identities,
  );
  const problems = virtualProblems(toSave);

  const run = async (work: () => Promise<string>) => {
    setBusy(true);
    setMessage(null);
    try {
      setMessage({ id: await work(), error: false });
    } catch (e) {
      setMessage({ id: errorMessageIdOf(e), error: true });
    } finally {
      setBusy(false);
    }
  };

  const refresh = () =>
    Promise.all([
      queryClient.invalidateQueries({ queryKey: queryKeys.virtualAccounts }),
      queryClient.invalidateQueries({ queryKey: queryKeys.accounts }),
      queryClient.invalidateQueries({ queryKey: queryKeys.allFolders }),
    ]);

  const save = (event: FormEvent) => {
    event.preventDefault();
    if (problems.length > 0) return;
    void run(async () => {
      const response = await client.request(
        isNew
          ? { type: "AddVirtualAccount", config: toSave }
          : { type: "UpdateVirtualAccount", config: toSave },
        "VirtualAccountSaved",
      );
      setConfig(response.config);
      await refresh();
      onDone(response.config.id);
      return "virtual-saved";
    });
  };

  const remove = () =>
    run(async () => {
      await client.request({ type: "RemoveVirtualAccount", account_id: config.id }, "Done");
      await refresh();
      onDone(null);
      return "virtual-removed";
    });

  const toggleMember = (id: string, on: boolean) =>
    setConfig({
      ...config,
      members: on
        ? accounts.map((a) => a.id).filter((a) => a === id || config.members.includes(a))
        : config.members.filter((m) => m !== id),
    });

  const setRule = (index: number, rule: IdentityRule) =>
    setConfig({
      ...config,
      identity_rules: config.identity_rules.map((old, i) => (i === index ? rule : old)),
    });

  return (
    <form
      data-part="virtual-editor.form"
      data-account={config.id || undefined}
      noValidate
      onSubmit={save}
    >
      <TextField
        labelId="virtual-name"
        name="name"
        value={config.name}
        onChange={(name) => setConfig({ ...config, name })}
      />
      <Section titleId="virtual-members">
        {accounts.map((account) => (
          <CheckField
            key={account.id}
            labelId="virtual-member"
            args={{ name: account.name }}
            name={`member-${account.id}`}
            checked={config.members.includes(account.id)}
            onChange={(on) => toggleMember(account.id, on)}
          />
        ))}
      </Section>
      <Section titleId="virtual-folders">
        <CheckField
          labelId="virtual-merge-roles"
          name="merge-roles"
          checked={config.merge_roles}
          onChange={(merge_roles) => setConfig({ ...config, merge_roles })}
        />
        <ul data-part="virtual-editor.mappings">
          {config.folder_mappings.map((mapping, index) => (
            <MappingEditor
              key={index}
              index={index}
              mapping={mapping}
              accounts={memberAccounts}
              onChange={(next) =>
                setConfig({
                  ...config,
                  folder_mappings: config.folder_mappings.map((old, i) =>
                    i === index ? next : old,
                  ),
                })
              }
              onRemove={() =>
                setConfig({
                  ...config,
                  folder_mappings: config.folder_mappings.filter((_, i) => i !== index),
                })
              }
            />
          ))}
        </ul>
        <button
          type="button"
          data-part="form.button"
          data-action="add-mapping"
          onClick={() =>
            setConfig({
              ...config,
              folder_mappings: [...config.folder_mappings, { name: "", sources: [] }],
            })
          }
        >
          {t("virtual-add-mapping")}
        </button>
      </Section>
      <Section titleId="virtual-identities">
        <SelectField
          labelId="virtual-default-identity"
          name="default-identity"
          value={config.default_identity ?? ""}
          choices={[{ value: "", label: "virtual-default-identity-auto" }, ...identityChoices]}
          onChange={(v) => setConfig({ ...config, default_identity: v === "" ? null : v })}
        />
        <ul data-part="virtual-editor.rules">
          {config.identity_rules.map((rule, index) => (
            <li key={index} data-part="virtual-editor.rule">
              <TextField
                labelId="virtual-rule-pattern"
                name={`rule-${index}-pattern`}
                hintId="virtual-rule-pattern-hint"
                value={rule.pattern}
                onChange={(pattern) => setRule(index, { ...rule, pattern: pattern.trim() })}
              />
              <SelectField
                labelId="virtual-rule-identity"
                name={`rule-${index}-identity`}
                value={rule.identity}
                choices={[{ value: "", label: "virtual-source-choose" }, ...identityChoices]}
                onChange={(identity) => setRule(index, { ...rule, identity })}
              />
              <button
                type="button"
                data-part="form.button"
                data-action="remove-rule"
                onClick={() =>
                  setConfig({
                    ...config,
                    identity_rules: config.identity_rules.filter((_, i) => i !== index),
                  })
                }
              >
                {t("virtual-remove-rule")}
              </button>
            </li>
          ))}
        </ul>
        <button
          type="button"
          data-part="form.button"
          data-action="add-rule"
          disabled={memberIdentities.length === 0}
          onClick={() =>
            setConfig({
              ...config,
              identity_rules: [
                ...config.identity_rules,
                { pattern: "", identity: memberIdentities[0]?.id ?? "" },
              ],
            })
          }
        >
          {t("virtual-add-rule")}
        </button>
      </Section>
      {problems.map((problem) => (
        <div key={problem} data-part="virtual-editor.problem" role="alert">
          {t(problem)}
        </div>
      ))}
      <div data-part="virtual-editor.actions">
        <button
          type="submit"
          data-part="form.button"
          data-action="save"
          data-variant="primary"
          disabled={busy || problems.length > 0}
        >
          {t(isNew ? "virtual-create" : "virtual-save")}
        </button>
        {!isNew && (
          <button
            type="button"
            data-part="form.button"
            data-action="delete"
            data-variant="danger"
            disabled={busy}
            onClick={() => void remove()}
          >
            {t("virtual-delete")}
          </button>
        )}
      </div>
    </form>
  );
}

/**
 * Virtual accounts: members, merging by role, hand-made folder mappings,
 * default identity and per-domain identity rules.
 */
export function VirtualAccountEditor({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const virtualAccounts = useVirtualAccounts();
  const accountsConfig = useAccountsConfig();
  const [selected, setSelected] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [notice, setNotice] = useState<Notice>(null);
  const list = virtualAccounts.data ?? [];
  const accounts = accountsConfig.data?.accounts ?? [];
  const current = creating ? null : (list.find((v) => v.id === selected) ?? list[0] ?? null);
  const error = virtualAccounts.error ?? accountsConfig.error;

  return (
    <section
      data-part="virtual-editor"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("virtual-label")}
    >
      <div data-part="virtual-editor.sidebar">
        <ul data-part="virtual-editor.list" aria-label={t("virtual-label")}>
          {list.map((va) => {
            const active = !creating && va.id === current?.id;
            return (
              <li key={va.id} data-part="virtual-editor.list.item">
                <button
                  type="button"
                  data-part="virtual-editor.account"
                  data-state={active ? "selected" : undefined}
                  aria-pressed={active}
                  onClick={() => {
                    setCreating(false);
                    setSelected(va.id);
                    setNotice(null);
                  }}
                >
                  {va.name}
                </button>
              </li>
            );
          })}
        </ul>
        <button
          type="button"
          data-part="form.button"
          data-action="new"
          disabled={accounts.length === 0}
          onClick={() => {
            setCreating(true);
            setNotice(null);
          }}
        >
          {t("virtual-new")}
        </button>
      </div>
      {error && (
        <div data-part="virtual-editor.error" role="alert">
          {t(errorMessageIdOf(error))}
        </div>
      )}
      {virtualAccounts.isSuccess && list.length === 0 && !creating && (
        <div data-part="virtual-editor.empty">{t("virtual-none")}</div>
      )}
      {accountsConfig.isSuccess && (creating || current) && (
        <VirtualForm
          key={creating ? "new" : (current?.id ?? "")}
          initial={creating || !current ? newVirtualAccount(accounts.map((a) => a.id)) : current}
          accounts={accounts}
          onDone={(id) => {
            setCreating(false);
            setSelected(id);
          }}
          setMessage={setNotice}
        />
      )}
      <NoticeView part="virtual-editor.message" notice={notice} />
    </section>
  );
}
