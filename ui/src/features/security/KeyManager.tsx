/**
 * Settings → Keys and certificates (layout widget `security-keys`): one
 * keyring for OpenPGP keys and S/MIME certificates with filters, details,
 * trust editing, import / export, generation, online lookup and the
 * Windows certificate store.
 */
import { useState } from "react";
import { errorMessageIdOf } from "../../api/client";
import type { CryptoKey, CryptoTechnology, KeyId } from "../../api/generated";
import { useLabel, useT } from "../../i18n/I18nProvider";
import type { WidgetProps } from "../../layout/widgetRegistry";
import { usePlatformFeature } from "../../state/platform";
import { CheckField, SelectField, TextField } from "../../widgets/forms";
import { KeyDetail } from "./KeyDetail";
import { GenerateTool, ImportTool, LookupTool, WindowsStoreTool } from "./KeyTools";
import { TECHNOLOGIES, formatFingerprint, keyState } from "./model";
import { useKeys } from "./queries";

/** The tools above the list, in button order. */
export const KEY_TOOLS = ["import", "generate", "lookup", "windows"] as const;
export type KeyTool = (typeof KEY_TOOLS)[number];

function KeyRow({
  keyInfo,
  active,
  onSelect,
}: {
  keyInfo: CryptoKey;
  active: boolean;
  onSelect: () => void;
}) {
  const t = useT();
  const state = keyState(keyInfo);
  return (
    <li data-part="security-keys.item">
      <button
        type="button"
        data-part="security-keys.key"
        data-state={active ? "selected" : undefined}
        data-key-state={state}
        data-technology={keyInfo.technology}
        data-own={keyInfo.has_secret ? "" : undefined}
        aria-pressed={active}
        onClick={onSelect}
      >
        <span data-part="security-keys.key.name">
          {keyInfo.user_ids[0] ?? formatFingerprint(keyInfo.fingerprint)}
        </span>
        <span data-part="security-keys.key.technology">
          {t(`security-technology-short-${keyInfo.technology}`)}
        </span>
        {keyInfo.has_secret && (
          <span data-part="security-keys.key.own">{t("security-keys-own")}</span>
        )}
        <span data-part="security-trust" data-trust={keyInfo.trust}>
          {t(`security-trust-${keyInfo.trust}`)}
        </span>
        {state !== "ok" && (
          <span data-part="security-keys.key.state" data-state={state}>
            {t(`security-key-state-short-${state}`)}
          </span>
        )}
        <code data-part="security-keys.key.fingerprint">
          {formatFingerprint(keyInfo.fingerprint.slice(-16))}
        </code>
      </button>
    </li>
  );
}

/** The `security-keys` widget. */
export function KeyManager({ node }: WidgetProps) {
  const t = useT();
  const label = useLabel();
  const [text, setText] = useState("");
  const [technology, setTechnology] = useState<CryptoTechnology | "">("");
  const [ownOnly, setOwnOnly] = useState(false);
  const [tool, setTool] = useState<KeyTool | null>(null);
  const [selected, setSelected] = useState<KeyId | null>(null);
  const keys = useKeys({
    technology: technology === "" ? null : technology,
    text: text.trim() === "" ? null : text.trim(),
    own_only: ownOnly,
  });
  const list = keys.data ?? [];
  const current = list.find((k) => k.id === selected) ?? list[0] ?? null;
  // ADR 0202: the Windows certificate store only where the core has it.
  const windowsStore = usePlatformFeature("windows-certificate-store");
  const tools = KEY_TOOLS.filter((k) => k !== "windows" || windowsStore);
  const onKeys = (added: CryptoKey[]) => {
    const first = added[0];
    if (first) {
      setSelected(first.id);
      setText("");
      setTechnology("");
      setOwnOnly(false);
    }
  };

  return (
    <section
      data-part="security-keys"
      data-node-id={node.id ?? undefined}
      aria-label={node.label ? label(node.label) : t("security-keys-label")}
    >
      <p data-part="security-keys.intro">{t("security-keys-intro")}</p>
      <div data-part="security-keys.tools" role="toolbar" aria-label={t("security-keys-tools")}>
        {tools.map((k) => (
          <button
            key={k}
            type="button"
            data-part="form.button"
            data-state={tool === k ? "selected" : undefined}
            data-action={`tool-${k}`}
            aria-pressed={tool === k}
            onClick={() => {
              setTool(tool === k ? null : k);
            }}
          >
            {t(`security-tool-${k}`)}
          </button>
        ))}
      </div>
      {tool === "import" && <ImportTool onKeys={onKeys} />}
      {tool === "generate" && <GenerateTool onKeys={onKeys} />}
      {tool === "lookup" && <LookupTool onKeys={onKeys} />}
      {tool === "windows" && <WindowsStoreTool onKeys={onKeys} />}
      <div data-part="security-keys.filters" role="search">
        <TextField
          labelId="security-keys-search"
          name="key-search"
          value={text}
          onChange={setText}
        />
        <SelectField
          labelId="security-keys-technology"
          name="key-technology"
          value={technology}
          onChange={(v) => {
            setTechnology(v as CryptoTechnology | "");
          }}
          choices={[
            { value: "", label: "security-keys-technology-all" },
            ...TECHNOLOGIES.map((tech) => ({ value: tech, label: `security-technology-${tech}` })),
          ]}
        />
        <CheckField
          labelId="security-keys-own-only"
          name="key-own-only"
          checked={ownOnly}
          onChange={setOwnOnly}
        />
      </div>
      {keys.isError && (
        <p data-part="security-keys.error" role="alert">
          {t(errorMessageIdOf(keys.error))}
        </p>
      )}
      {keys.isSuccess && list.length === 0 && (
        <p data-part="security-keys.empty">{t("security-keys-empty")}</p>
      )}
      {list.length > 0 && (
        <ul data-part="security-keys.list" aria-label={t("security-keys-list")}>
          {list.map((k) => (
            <KeyRow
              key={k.id}
              keyInfo={k}
              active={k.id === current?.id}
              onSelect={() => {
                setSelected(k.id);
              }}
            />
          ))}
        </ul>
      )}
      {current && (
        <KeyDetail
          key={`${current.id}:${current.trust}`}
          keyInfo={current}
          onDeleted={() => {
            setSelected(null);
          }}
        />
      )}
    </section>
  );
}
