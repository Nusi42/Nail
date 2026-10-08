/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en/security.ftl`; ids built at
 * runtime are listed by `runtimeIds`.
 */
import {
  PIN_PROMPT_CLOSE_REASON_MESSAGE_IDS,
  PIN_PURPOSE_MESSAGE_IDS,
  RECOVERY_KEY_ISSUE_MESSAGE_IDS,
  SMARTCARD_ERROR_MESSAGE_IDS,
  SMARTCARD_KEY_USAGE_MESSAGE_IDS,
  SMARTCARD_OBJECT_KIND_MESSAGE_IDS,
  SMARTCARD_SETTINGS_ISSUE_MESSAGE_IDS,
  VAULT_PHASE_VALUES,
} from "../../api/generated";
import type {
  CertificateChain,
  DecryptionState,
  HeaderProtection,
  KeySource,
  RecipientKeysReport,
  RevocationState,
  SecretStorage,
  SignatureState,
  VaultPhase,
  VaultState,
} from "../../api/generated";
import { AUTOCRYPT_IMPORT_DIALOG } from "./Autocrypt";
import { EXPIRY_CHOICES, WINDOWS_STORES } from "./KeyTools";
import { KEY_TOOLS } from "./KeyManager";
import { REMEMBER_CHOICES } from "./MessageSecurity";
import {
  ATTACHMENT_RISKS,
  AUTH_VERDICTS,
  ENCRYPTION_VERDICTS,
  PREFERENCES,
  SIGNATURE_VERDICTS,
  TECHNOLOGIES,
  TRUST_DECISIONS,
  TRUST_LEVELS,
} from "./model";
import { LOOKUP_POLICIES, LOOKUP_SOURCES } from "./SecuritySettings";
import { METHODS } from "./MessageSecurity";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["security-"];

/** Literals with those prefixes that are not message ids (part names, widget kinds). */
export const notMessages: readonly string[] = [
  "security-attachment-warning",
  "security-autocrypt",
  "security-badge",
  "security-chain",
  "security-compose",
  "security-composer",
  "security-details",
  "security-fingerprint",
  "security-generate",
  "security-identities",
  "security-identity",
  "security-import",
  "security-index-settings",
  "security-key",
  "security-keys",
  "security-lookup",
  "security-lookup-settings",
  "security-message",
  "security-protection",
  "security-settings",
  "security-tool",
  "security-trust",
  "security-unlock",
  "security-vault",
  "security-warnings",
  "security-windows",
  // M4c widget kinds.
  "security-recovery-key",
  "security-smartcards",
  // `KeySource` value that happens to match the shared `setup-` prefix.
  "setup-message",
];

const SIGNATURE_STATES: SignatureState[] = [
  "valid",
  "invalid",
  "unknown-key",
  "key-expired",
  "key-revoked",
  "unsupported",
  "error",
];
const DECRYPTION_STATES: DecryptionState[] = [
  "decrypted",
  "no-key",
  "locked",
  "failed",
  "unsupported",
];
const HEADERS: HeaderProtection[] = ["none", "protected", "mismatch"];
const CHAIN_STATES: CertificateChain["state"][] = [
  "valid",
  "untrusted-root",
  "incomplete",
  "expired",
  "wrong-usage",
  "invalid",
];
const REVOCATION: RevocationState[] = ["not-checked", "good", "revoked"];
const SOURCES: KeySource[] = [
  "generated",
  "imported",
  "pkcs12",
  "windows-store",
  "autocrypt",
  "autocrypt-gossip",
  "setup-message",
  "wkd",
  "keyserver",
  "message",
  "smartcard",
];
const STORAGE: SecretStorage[] = ["keychain", "passphrase", "windows-store", "smartcard"];
const VAULT_STATES: VaultState[] = ["plain", "encrypting", "encrypted", "decrypting"];
const VAULT_PHASES: readonly VaultPhase[] = VAULT_PHASE_VALUES;
const AUTOCRYPT: RecipientKeysReport["autocrypt"][] = ["discourage", "available", "encrypt"];
const RECIPIENT_STATES = [
  "usable",
  "expired",
  "revoked",
  "rejected",
  "cannot-encrypt",
  "invalid-chain",
  "missing",
];
const LOOKUP_STATES = ["found", "not-found", "failed"];
const KEY_STATES = ["revoked", "expired", "rejected"];
const TONES = ["good", "neutral", "warning", "danger"];
/** Settings tabs and actions the default layout labels (`@security.…`). */
const LAYOUT_LABELS = [
  "security-settings-tab-keys",
  "security-settings-tab-identities",
  "security-settings-tab-protection",
  "security-settings-tab-autocrypt",
  "security-action-autocrypt-import",
];

/** M4c dialogs (titles `window-<name>`). */
const M4C_DIALOGS = [
  "security-recovery-create",
  "security-recovery-verify",
  "security-recovery-restore",
  "smartcard-pin",
];

/** Message ids built at runtime (e.g. `security-trust-<level>`). */
export function runtimeIds(): string[] {
  return [
    ...TRUST_LEVELS.map((l) => `security-trust-${l}`),
    ...TRUST_DECISIONS.flatMap((d) => [`security-decision-${d}`, `security-decision-${d}-hint`]),
    ...TECHNOLOGIES.flatMap((t) => [
      `security-technology-${t}`,
      `security-technology-short-${t}`,
      `security-compose-block-no-own-key-${t}`,
    ]),
    ...PREFERENCES.map((p) => `security-preference-${p}`),
    ...SIGNATURE_VERDICTS.filter((v) => v !== "none").map((v) => `security-badge-sig-${v}`),
    ...ENCRYPTION_VERDICTS.filter((v) => v !== "none").map((v) => `security-badge-enc-${v}`),
    ...SIGNATURE_STATES.map((s) => `security-signature-${s}`),
    ...DECRYPTION_STATES.map((s) => `security-encryption-${s}`),
    ...HEADERS.map((h) => `security-headers-${h}`),
    ...CHAIN_STATES.map((s) => `security-chain-${s}`),
    ...REVOCATION.map((r) => `security-revocation-${r}`),
    ...AUTH_VERDICTS.flatMap((v) => [`security-auth-badge-${v}`, `security-auth-verdict-${v}`]),
    ...TONES.map((t) => `security-auth-summary-${t}`),
    ...METHODS.map((m) => `security-auth-method-${m}`),
    ...ATTACHMENT_RISKS.flatMap((r) => [`security-risk-${r}`, `security-risk-short-${r}`]),
    ...SOURCES.map((s) => `security-source-${s}`),
    ...STORAGE.map((s) => `security-secret-${s}`),
    ...KEY_STATES.flatMap((s) => [`security-key-state-${s}`, `security-key-state-short-${s}`]),
    ...KEY_TOOLS.map((k) => `security-tool-${k}`),
    ...EXPIRY_CHOICES.map((d) => `security-expiry-${d === "" ? "never" : d}`),
    ...REMEMBER_CHOICES.map((m) => `security-unlock-remember-${m === "" ? "session" : m}`),
    ...WINDOWS_STORES.map((s) => `security-windows-store-${s}`),
    ...["keychain", "passphrase"].map((p) => `security-protection-${p}-hint`),
    ...LOOKUP_POLICIES.flatMap((p) => [
      `security-lookup-policy-${p}`,
      `security-lookup-policy-${p}-hint`,
    ]),
    ...LOOKUP_SOURCES.map((s) => `security-lookup-source-${s}`),
    ...VAULT_STATES.map((s) => `security-vault-state-${s}`),
    ...VAULT_PHASES.map((p) => `security-vault-phase-${p}`),
    ...AUTOCRYPT.map((a) => `security-compose-autocrypt-${a}`),
    ...RECIPIENT_STATES.map((s) => `security-compose-key-${s}`),
    ...LOOKUP_STATES.map((s) => `security-compose-lookup-${s}`),
    `window-${AUTOCRYPT_IMPORT_DIALOG}`,
    ...LAYOUT_LABELS,
    // M4c: recovery key, smartcards, PIN prompts (contract texts in core-vault.ftl).
    ...M4C_DIALOGS.map((d) => `window-${d}`),
    ...Object.values(RECOVERY_KEY_ISSUE_MESSAGE_IDS),
    "recovery-key-wrong",
    "vault-not-encrypted",
    ...Object.values(PIN_PURPOSE_MESSAGE_IDS),
    ...Object.values(PIN_PROMPT_CLOSE_REASON_MESSAGE_IDS),
    ...Object.values(SMARTCARD_OBJECT_KIND_MESSAGE_IDS),
    ...Object.values(SMARTCARD_KEY_USAGE_MESSAGE_IDS),
    ...Object.values(SMARTCARD_ERROR_MESSAGE_IDS),
    ...Object.values(SMARTCARD_SETTINGS_ISSUE_MESSAGE_IDS),
  ];
}
