/**
 * The vault recovery key (M4c WP124; contract ADR 0094):
 *
 * - widget `security-recovery-key` — whether a key exists, when it was made
 *   and last checked, Create / Check / Remove;
 * - dialog `security-recovery-create` — explains what the key is for,
 *   optionally writes a recovery-kit file, creates the key
 *   (`CreateRecoveryKey`), **shows it once** with Copy and Print, and asks
 *   the user to type it back (`VerifyRecoveryKey`) before it is done;
 * - dialog `security-recovery-verify` — checks a stored key any time;
 * - dialog `security-recovery-restore` — when the data is encrypted but
 *   its key is gone from the keychain (`VaultStatus.needs_recovery`), puts
 *   it back with the recovery key (`RestoreVaultKey`); the effect
 *   `RecoveryGate` opens it once per session in that state.
 *
 * Typed keys are checked locally first (`parseRecoveryKey`): a typo is
 * reported at once and never sent as a wrong key. The key itself lives only
 * in the dialog's state; closing the dialog forgets it.
 */
import { useQueryClient } from "@tanstack/react-query";
import { useEffect, useId, useRef, useState } from "react";
import { errorMessageOf } from "../../api/client";
import type { LocalizedMessage, RecoveryKeyStatus } from "../../api/generated";
import { useT } from "../../i18n/I18nProvider";
import { useDateFormat } from "../../i18n/format";
import type { DialogProps } from "../../layout/dialogRegistry";
import { useClient } from "../../state/queries";
import { useUiStoreApi } from "../../state/uiStore";
import { PathField, useSavePath } from "../portability/PathField";
import { printSheet } from "../print";
import { securityKeys, useVaultStatus } from "./queries";
import { parseRecoveryKey, recoveryIssueId, recoveryKeyGroups } from "./recoveryKeyText";
import { useBusyAction } from "../../state/busyAction";
import { CloseButton } from "../../widgets/CloseButton";

export const RECOVERY_WIDGET = "security-recovery-key";
export const RECOVERY_CREATE_DIALOG = "security-recovery-create";
export const RECOVERY_VERIFY_DIALOG = "security-recovery-verify";
export const RECOVERY_RESTORE_DIALOG = "security-recovery-restore";

const KIT_FILTERS = [{ label_id: "security-recovery-kit-filter", extensions: ["txt"] }];

/** Keeps the cached vault status's recovery part current. */
function useUpdateRecovery(): (recovery: RecoveryKeyStatus) => void {
  const queryClient = useQueryClient();
  return (recovery) =>
    queryClient.setQueryData(
      securityKeys.vault,
      (old: ReturnType<typeof useVaultStatus>["data"]) => (old ? { ...old, recovery } : old),
    );
}

/** The key shown in groups, with a text alternative that reads it whole. */
export function RecoveryKeyDisplay({ value }: { value: string }) {
  const t = useT();
  return (
    <figure data-part="recovery-key" aria-label={t("security-recovery-key-label")}>
      <code data-part="recovery-key.value" aria-label={value}>
        {recoveryKeyGroups(value).map((group, i) => (
          <span key={i} data-part="recovery-key.group">
            {group}
          </span>
        ))}
      </code>
    </figure>
  );
}

/** The printed recovery kit (a local sheet, no core job). */
function RecoveryKit({ value, created }: { value: string; created: string }) {
  const t = useT();
  return (
    <article data-part="print.recovery-kit">
      <h1 data-part="print.subject">{t("security-recovery-kit-title")}</h1>
      <p data-part="print.text">{t("security-recovery-kit-text", { created })}</p>
      <RecoveryKeyDisplay value={value} />
      <p data-part="print.text">{t("security-recovery-kit-keep")}</p>
    </article>
  );
}

/** A field for typing a recovery key, with the local typo check. */
export function RecoveryKeyInput({
  value,
  onChange,
  label,
}: {
  value: string;
  onChange: (value: string) => void;
  label: string;
}) {
  const t = useT();
  const id = useId();
  const parsed = parseRecoveryKey(value);
  // Complain about the length only once something key-sized is typed.
  const typed = value.replace(/[\s-]/g, "").length;
  const issue =
    parsed.ok || value.trim() === ""
      ? null
      : parsed.issue === "wrong-length" && typed < 56
        ? null
        : parsed.issue;
  return (
    <div data-part="form.field" data-field="recovery-key">
      <label data-part="form.label" htmlFor={id}>
        {label}
      </label>
      <input
        id={id}
        data-part="form.input"
        data-field="recovery-key"
        type="text"
        autoComplete="off"
        autoCapitalize="characters"
        spellCheck={false}
        value={value}
        aria-invalid={issue !== null || undefined}
        aria-describedby={`${id}-hint`}
        onChange={(e) => {
          onChange(e.target.value);
        }}
      />
      <p data-part="form.hint" id={`${id}-hint`} data-tone={issue ? "danger" : undefined}>
        {t(issue ? recoveryIssueId(issue) : "security-recovery-input-hint")}
      </p>
    </div>
  );
}

function ProblemText({ problem }: { problem: LocalizedMessage }) {
  const t = useT();
  return (
    <p data-part="form.error" role="alert">
      {t(problem.id, problem.args)}
    </p>
  );
}

type CreateStep = "intro" | "show" | "verify" | "done";

/** The create flow's content. */
export function RecoveryKeyCreate({ close }: { close: () => void }) {
  const t = useT();
  const client = useClient();
  const ui = useUiStoreApi();
  const format = useDateFormat();
  const vault = useVaultStatus();
  const updateRecovery = useUpdateRecovery();
  const [step, setStep] = useState<CreateStep>("intro");
  const [saveKit, setSaveKit] = useState(false);
  // `overwrite`: the Save dialog confirmed replacing an existing file.
  const { path: kitPath, overwrite, onChange: setKitPath } = useSavePath();
  const [key, setKey] = useState<string | null>(null);
  const [savedTo, setSavedTo] = useState<string | null>(null);
  const [createdAt, setCreatedAt] = useState<string | null>(null);
  const [stored, setStored] = useState(false);
  const [copied, setCopied] = useState(false);
  const [typed, setTyped] = useState("");
  const { busy, problem, setProblem, run } = useBusyAction();
  const replacing = vault.data?.recovery.configured ?? false;

  const create = async () => {
    await run(async () => {
      const answer = await client.request(
        {
          type: "CreateRecoveryKey",
          save_to: saveKit && kitPath.trim() !== "" ? kitPath.trim() : null,
          overwrite: saveKit && overwrite,
        },
        "RecoveryKeyCreated",
      );
      setKey(answer.key);
      setSavedTo(answer.saved_to);
      setCreatedAt(answer.status.created_at);
      updateRecovery(answer.status);
      setStep("show");
    });
  };

  const verify = async () => {
    await run(async () => {
      const answer = await client.request(
        { type: "VerifyRecoveryKey", key: typed },
        "RecoveryKeyVerified",
      );
      updateRecovery(answer.status);
      if (answer.valid) {
        setStep("done");
        setKey(null);
      } else {
        setProblem({ id: "security-recovery-verify-mismatch" });
      }
    });
  };

  const copy = async () => {
    if (!key) return;
    try {
      await navigator.clipboard.writeText(key);
      setCopied(true);
    } catch {
      setProblem({ id: "security-recovery-copy-failed" });
    }
  };

  return (
    <div data-part="recovery" data-step={step}>
      {step === "intro" && (
        <>
          <p data-part="recovery.text">{t("security-recovery-intro")}</p>
          {replacing && (
            <p data-part="form.hint" data-tone="warning" data-field="replace">
              {t("security-recovery-replace-warning")}
            </p>
          )}
          <label data-part="form.check" data-field="save-kit">
            <input
              data-part="form.checkbox"
              type="checkbox"
              checked={saveKit}
              onChange={(e) => {
                setSaveKit(e.target.checked);
              }}
            />
            <span data-part="form.check.label">{t("security-recovery-save-kit")}</span>
          </label>
          {saveKit && (
            <PathField
              labelId="security-recovery-kit-file"
              field="kit-path"
              value={kitPath}
              onChange={setKitPath}
              kind="save-file"
              titleId="security-recovery-kit-pick-title"
              filters={KIT_FILTERS}
              defaultPath={t("security-recovery-kit-default-name")}
            />
          )}
          {problem && <ProblemText problem={problem} />}
          <div data-part="wizard.buttons">
            <button
              type="button"
              data-part="form.button"
              data-variant="primary"
              data-action="create"
              disabled={busy || (saveKit && kitPath.trim() === "")}
              onClick={() => void create()}
            >
              {t(replacing ? "security-recovery-replace" : "security-recovery-create")}
            </button>
          </div>
        </>
      )}
      {step === "show" && key && (
        <>
          <p data-part="recovery.text" role="status">
            {t("security-recovery-show-once")}
          </p>
          <RecoveryKeyDisplay value={key} />
          {savedTo && (
            <p data-part="form.hint" data-field="saved-to">
              {t("security-recovery-kit-saved", { path: savedTo })}
            </p>
          )}
          <div data-part="wizard.buttons">
            <button
              type="button"
              data-part="form.button"
              data-action="copy"
              onClick={() => void copy()}
            >
              {t(copied ? "security-recovery-copied" : "security-recovery-copy")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-action="print"
              onClick={() => {
                printSheet(
                  ui,
                  <RecoveryKit value={key} created={createdAt ? format.dateTime(createdAt) : ""} />,
                );
              }}
            >
              {t("common-print")}
            </button>
          </div>
          <label data-part="form.check" data-field="stored">
            <input
              data-part="form.checkbox"
              type="checkbox"
              checked={stored}
              onChange={(e) => {
                setStored(e.target.checked);
              }}
            />
            <span data-part="form.check.label">{t("security-recovery-stored")}</span>
          </label>
          {problem && <ProblemText problem={problem} />}
          <div data-part="wizard.buttons">
            <button
              type="button"
              data-part="form.button"
              data-variant="primary"
              data-action="next"
              disabled={!stored}
              onClick={() => {
                setProblem(null);
                setStep("verify");
              }}
            >
              {t("security-recovery-next")}
            </button>
          </div>
        </>
      )}
      {step === "verify" && (
        <>
          <p data-part="recovery.text">{t("security-recovery-verify-intro")}</p>
          <RecoveryKeyInput
            value={typed}
            onChange={setTyped}
            label={t("security-recovery-input")}
          />
          {problem && <ProblemText problem={problem} />}
          <div data-part="wizard.buttons">
            <button
              type="button"
              data-part="form.button"
              data-action="back"
              onClick={() => {
                setProblem(null);
                setStep("show");
              }}
            >
              {t("security-recovery-show-again")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-variant="primary"
              data-action="verify"
              disabled={busy || !parseRecoveryKey(typed).ok}
              onClick={() => void verify()}
            >
              {t("security-recovery-verify")}
            </button>
          </div>
        </>
      )}
      {step === "done" && (
        <>
          <p data-part="recovery.text" role="status" data-tone="success">
            {t("security-recovery-done")}
          </p>
          <div data-part="wizard.buttons">
            <CloseButton onClose={close} action="close" />
          </div>
        </>
      )}
    </div>
  );
}

/** Checks a stored recovery key. */
export function RecoveryKeyVerify({ close }: { close: () => void }) {
  const t = useT();
  const client = useClient();
  const updateRecovery = useUpdateRecovery();
  const [typed, setTyped] = useState("");
  const [result, setResult] = useState<boolean | null>(null);
  const { busy, problem, run } = useBusyAction();

  const verify = async () => {
    await run(async () => {
      const answer = await client.request(
        { type: "VerifyRecoveryKey", key: typed },
        "RecoveryKeyVerified",
      );
      updateRecovery(answer.status);
      setResult(answer.valid);
    });
  };

  return (
    <div data-part="recovery" data-step="check">
      <p data-part="recovery.text">{t("security-recovery-check-intro")}</p>
      <RecoveryKeyInput
        value={typed}
        onChange={(v) => {
          setTyped(v);
          setResult(null);
        }}
        label={t("security-recovery-input")}
      />
      {result !== null && (
        <p data-part="recovery.result" role="status" data-tone={result ? "success" : "danger"}>
          {t(result ? "security-recovery-check-valid" : "security-recovery-check-invalid")}
        </p>
      )}
      {problem && <ProblemText problem={problem} />}
      <div data-part="wizard.buttons">
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          data-action="verify"
          disabled={busy || !parseRecoveryKey(typed).ok}
          onClick={() => void verify()}
        >
          {t("security-recovery-verify")}
        </button>
        <CloseButton onClose={close} action="close" />
      </div>
    </div>
  );
}

/** Puts the data key back with the recovery key. */
export function RecoveryKeyRestore({ close }: { close: () => void }) {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const vault = useVaultStatus();
  const [typed, setTyped] = useState("");
  const [done, setDone] = useState(false);
  const [restartNeeded, setRestartNeeded] = useState(false);
  const { busy, problem, run } = useBusyAction();
  const needed = vault.data?.needs_recovery ?? false;

  const restore = async () => {
    await run(async () => {
      const answer = await client.request({ type: "RestoreVaultKey", key: typed }, "VaultStatus");
      queryClient.setQueryData(securityKeys.vault, answer.status);
      setTyped("");
      // A locked data directory opens after a restart (WP125, ADR 0099).
      setRestartNeeded(answer.status.localized?.id === "vault-restart-needed");
      setDone(true);
    });
  };

  if (done) {
    return (
      <div data-part="recovery" data-step="restored">
        <p data-part="recovery.text" role="status" data-tone="success">
          {t(restartNeeded ? "vault-restart-needed" : "security-recovery-restored")}
        </p>
        <div data-part="wizard.buttons">
          <CloseButton onClose={close} action="close" />
        </div>
      </div>
    );
  }
  return (
    <div data-part="recovery" data-step="restore">
      <p data-part="recovery.text">
        {t(needed ? "security-recovery-restore-needed" : "security-recovery-restore-intro")}
      </p>
      <RecoveryKeyInput value={typed} onChange={setTyped} label={t("security-recovery-input")} />
      {problem && <ProblemText problem={problem} />}
      <div data-part="wizard.buttons">
        <button
          type="button"
          data-part="form.button"
          data-variant="primary"
          data-action="restore"
          disabled={busy || !parseRecoveryKey(typed).ok}
          onClick={() => void restore()}
        >
          {t("security-recovery-restore")}
        </button>
      </div>
      <p data-part="form.hint">{t("security-recovery-restore-lost")}</p>
    </div>
  );
}

/** Widget `security-recovery-key`. */
export function RecoveryKeyWidget() {
  const t = useT();
  const client = useClient();
  const queryClient = useQueryClient();
  const ui = useUiStoreApi();
  const format = useDateFormat();
  const vault = useVaultStatus();
  const [confirmRemove, setConfirmRemove] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const status = vault.data;
  if (!status) return null;
  const recovery = status.recovery;
  const encrypted = status.state === "encrypted";

  const remove = async () => {
    setProblem(null);
    try {
      const answer = await client.request({ type: "RemoveRecoveryKey" }, "VaultStatus");
      queryClient.setQueryData(securityKeys.vault, answer.status);
      setConfirmRemove(false);
    } catch (error) {
      setProblem(errorMessageOf(error));
    }
  };

  return (
    <section data-part="recovery-settings" data-state={recovery.configured ? "configured" : "none"}>
      <h3 data-part="form.section.title">{t("security-recovery-title")}</h3>
      <p data-part="form.hint">{t("security-recovery-what")}</p>
      {status.needs_recovery && (
        <p data-part="form.hint" data-tone="danger" role="alert">
          {t("security-recovery-needed-banner")}
        </p>
      )}
      {!encrypted && !status.needs_recovery ? (
        <p data-part="form.hint" data-field="not-encrypted">
          {t("security-recovery-not-encrypted")}
        </p>
      ) : recovery.configured ? (
        <dl data-part="recovery-settings.facts">
          <div data-part="recovery-settings.fact">
            <dt data-part="recovery-settings.fact.label">{t("security-recovery-created")}</dt>
            <dd data-part="recovery-settings.fact.value">
              {recovery.created_at
                ? format.dateTime(recovery.created_at)
                : t("security-recovery-unknown")}
            </dd>
          </div>
          <div data-part="recovery-settings.fact">
            <dt data-part="recovery-settings.fact.label">{t("security-recovery-checked")}</dt>
            <dd data-part="recovery-settings.fact.value">
              {recovery.verified_at
                ? format.dateTime(recovery.verified_at)
                : t("security-recovery-never-checked")}
            </dd>
          </div>
        </dl>
      ) : (
        <p data-part="form.hint" data-tone="warning" data-field="missing">
          {t("security-recovery-missing")}
        </p>
      )}
      <div data-part="wizard.buttons">
        {status.needs_recovery ? (
          <button
            type="button"
            data-part="form.button"
            data-variant="primary"
            data-action="restore"
            onClick={() => {
              ui.getState().openWindow(RECOVERY_RESTORE_DIALOG);
            }}
          >
            {t("security-recovery-restore")}
          </button>
        ) : (
          encrypted && (
            <>
              <button
                type="button"
                data-part="form.button"
                data-variant={recovery.configured ? undefined : "primary"}
                data-action="create"
                onClick={() => {
                  ui.getState().openWindow(RECOVERY_CREATE_DIALOG);
                }}
              >
                {t(recovery.configured ? "security-recovery-replace" : "security-recovery-create")}
              </button>
              {recovery.configured && (
                <button
                  type="button"
                  data-part="form.button"
                  data-action="check"
                  onClick={() => {
                    ui.getState().openWindow(RECOVERY_VERIFY_DIALOG);
                  }}
                >
                  {t("security-recovery-check")}
                </button>
              )}
              {recovery.configured && !confirmRemove && (
                <button
                  type="button"
                  data-part="form.button"
                  data-variant="danger"
                  data-action="remove"
                  onClick={() => {
                    setConfirmRemove(true);
                  }}
                >
                  {t("security-recovery-remove")}
                </button>
              )}
            </>
          )
        )}
      </div>
      {confirmRemove && (
        <div
          data-part="recovery-settings.confirm"
          role="group"
          aria-label={t("security-recovery-remove")}
        >
          <p data-part="form.hint" data-tone="warning">
            {t("security-recovery-remove-warning")}
          </p>
          <div data-part="wizard.buttons">
            <button
              type="button"
              data-part="form.button"
              data-variant="danger"
              data-action="confirm-remove"
              onClick={() => void remove()}
            >
              {t("security-recovery-remove-confirm")}
            </button>
            <button
              type="button"
              data-part="form.button"
              data-action="keep"
              onClick={() => {
                setConfirmRemove(false);
              }}
            >
              {t("security-recovery-keep")}
            </button>
          </div>
        </div>
      )}
      {problem && <ProblemText problem={problem} />}
    </section>
  );
}

export function RecoveryCreateDialog({ close }: DialogProps) {
  return <RecoveryKeyCreate close={close} />;
}

export function RecoveryVerifyDialog({ close }: DialogProps) {
  return <RecoveryKeyVerify close={close} />;
}

export function RecoveryRestoreDialog({ close }: DialogProps) {
  return <RecoveryKeyRestore close={close} />;
}

/**
 * Effect: when the encrypted data's key is missing from the keychain
 * (`needs_recovery`), opens the restore dialog once per session.
 */
export function RecoveryGate() {
  const vault = useVaultStatus();
  const ui = useUiStoreApi();
  const offered = useRef(false);
  const needed = vault.data?.needs_recovery ?? false;
  useEffect(() => {
    if (!needed || offered.current) return;
    offered.current = true;
    ui.getState().openWindow(RECOVERY_RESTORE_DIALOG);
  }, [needed, ui]);
  return null;
}
