/**
 * Feature slot of M2a WP34 (security UI, ADR 0029 / ADR 0033): key
 * and certificate manager, per-identity crypto settings, composer sign /
 * encrypt toggles with recipient key status, the message security and
 * auth-results badges with details, link and attachment warnings in the
 * reader, the encryption-at-rest settings with progress, the Autocrypt
 * setup flow.
 *
 * Only WP34 edits the files in this directory (plus
 * `crates/nail-config/src/widgets/security.rs` for widget kinds and
 * `locales/{en,de}/security.ftl`). The central registry (`../registry.ts`)
 * merges these exports into the app; see `../README.md`.
 *
 * M4c (WP124, ADR 0094): the vault recovery key (widget
 * `security-recovery-key`, dialogs `security-recovery-create` / `-verify` /
 * `-restore`, effect `RecoveryGate`), smartcards (widget
 * `security-smartcards`) and the PIN dialog (`smartcard-pin`, opened by
 * `PinRequested`).
 *
 * Components for other packages to mount (the composer, the reader, the
 * account settings) are re-exported at the end of this file.
 */
import type { MessageBody } from "../../api/generated";
import type { ActionContext } from "../../actions/registry";
import { queryKeys } from "../../state/queries";
import type { ActionInfo } from "../../actions/palette";
import type { Feature, FeatureEventHandler } from "../types";
import { AUTOCRYPT_IMPORT_DIALOG, AutocryptImportDialog, AutocryptSetup } from "./Autocrypt";
import { ComposerSecurityWidget } from "./ComposerSecurity";
import { IdentityCryptoSettings } from "./IdentityCrypto";
import { KeyManager } from "./KeyManager";
import { PIN_DIALOG, PinPromptDialog, handlePinEvent } from "./PinPrompt";
import {
  RECOVERY_CREATE_DIALOG,
  RECOVERY_RESTORE_DIALOG,
  RECOVERY_VERIFY_DIALOG,
  RECOVERY_WIDGET,
  RecoveryCreateDialog,
  RecoveryGate,
  RecoveryKeyWidget,
  RecoveryRestoreDialog,
  RecoveryVerifyDialog,
} from "./RecoveryKey";
import { SMARTCARDS_WIDGET, SmartcardsWidget } from "./Smartcards";
import { SecurityMessageWidget } from "./MessageSecurity";
import { applyVaultStatus, securityKeys } from "./queries";
import { SecuritySettingsWidget } from "./SecuritySettings";
import { SecurityWarningsWidget } from "./Warnings";

/** Layout widget kinds → components. */
export const widgets: Feature["widgets"] = {
  "security-keys": KeyManager,
  "security-identities": IdentityCryptoSettings,
  "security-settings": SecuritySettingsWidget,
  "security-autocrypt": AutocryptSetup,
  "security-message": SecurityMessageWidget,
  "security-warnings": SecurityWarningsWidget,
  "security-composer": ComposerSecurityWidget,
  [RECOVERY_WIDGET]: RecoveryKeyWidget,
  [SMARTCARDS_WIDGET]: SmartcardsWidget,
};

/** The MIME type of an Autocrypt Setup Message's attachment (Autocrypt 1.1, 4.4). */
export const AUTOCRYPT_SETUP_MIME = "application/autocrypt-setup";
/** The armour header of a setup message's payload (Autocrypt 1.1, 4.4). */
const AUTOCRYPT_SETUP_ARMOR = "Passphrase-Format: numeric9x4";

/**
 * Whether the selected message (as the reader loaded it) is an Autocrypt
 * Setup Message: an `application/autocrypt-setup` attachment, or the
 * payload's armour in the text.
 */
export function isAutocryptSetupMessage(ctx: ActionContext): boolean {
  const id = ctx.store.getState().selectedMessageId;
  if (id === null) return false;
  const body = ctx.queryClient.getQueryData<MessageBody>(queryKeys.message(id));
  if (!body) return false;
  return (
    body.attachments.some(
      (a) => a.mime.toLowerCase().split(";")[0]?.trim() === AUTOCRYPT_SETUP_MIME,
    ) ||
    // The core also finds a payload in the text (no attachment).
    body.text.includes(AUTOCRYPT_SETUP_ARMOR)
  );
}

/** M4c action ids. */
export const RECOVERY_CREATE_ACTION = "security.recovery-key.create";
export const RECOVERY_CHECK_ACTION = "security.recovery-key.check";
export const RECOVERY_RESTORE_ACTION = "security.recovery-key.restore";

/** Action ids → handlers (prefix `security.`). */
export const actions: Feature["actions"] = {
  /** Imports the selected Autocrypt setup message (asks for the setup code). */
  "security.autocrypt.import": {
    run: (ctx) => {
      ctx.store.getState().openWindow(AUTOCRYPT_IMPORT_DIALOG);
    },
    // Only for Autocrypt Setup Messages (M5b review: not in More for every mail).
    enabled: (ctx) => isAutocryptSetupMessage(ctx),
  },
  /** Creates (or replaces) the vault recovery key. */
  [RECOVERY_CREATE_ACTION]: {
    run: (ctx) => {
      ctx.store.getState().openWindow(RECOVERY_CREATE_DIALOG);
    },
  },
  /** Checks a stored recovery key. */
  [RECOVERY_CHECK_ACTION]: {
    run: (ctx) => {
      ctx.store.getState().openWindow(RECOVERY_VERIFY_DIALOG);
    },
  },
  /** Puts the data key back with the recovery key. */
  [RECOVERY_RESTORE_ACTION]: {
    run: (ctx) => {
      ctx.store.getState().openWindow(RECOVERY_RESTORE_DIALOG);
    },
  },
};

/** Command-palette descriptions of the M4c actions (`security.ftl`). */
export const actionInfo: Readonly<Record<string, ActionInfo>> = {
  [RECOVERY_CREATE_ACTION]: {
    title: "security-action-recovery-create",
    keywords: "security-action-recovery-keywords",
    availability: ["desktop"],
  },
  [RECOVERY_CHECK_ACTION]: {
    title: "security-action-recovery-check",
    keywords: "security-action-recovery-keywords",
    availability: ["desktop"],
  },
  [RECOVERY_RESTORE_ACTION]: {
    title: "security-action-recovery-restore",
    keywords: "security-action-recovery-keywords",
    availability: ["desktop"],
  },
};

/** Named dialogs (opened with `window.open "<name>"`). */
export const dialogs: Feature["dialogs"] = {
  [AUTOCRYPT_IMPORT_DIALOG]: AutocryptImportDialog,
  [RECOVERY_CREATE_DIALOG]: RecoveryCreateDialog,
  [RECOVERY_VERIFY_DIALOG]: RecoveryVerifyDialog,
  [RECOVERY_RESTORE_DIALOG]: RecoveryRestoreDialog,
  [PIN_DIALOG]: PinPromptDialog,
};

/**
 * The window model of this feature's windows (M5c, ADR 0116): tool windows
 * are non-modal with their default and minimum sizes; only confirmations
 * (`confirmation: true`) are modal. The layout's `window` properties win.
 */
export const windows: NonNullable<Feature["windows"]> = {
  [AUTOCRYPT_IMPORT_DIALOG]: {
    size: { width: 560, height: 480 },
    min_size: { width: 420, height: 360 },
  },
  [RECOVERY_CREATE_DIALOG]: {
    size: { width: 640, height: 560 },
    min_size: { width: 480, height: 420 },
  },
  [RECOVERY_VERIFY_DIALOG]: {
    size: { width: 560, height: 420 },
    min_size: { width: 420, height: 320 },
  },
  [RECOVERY_RESTORE_DIALOG]: {
    size: { width: 640, height: 560 },
    min_size: { width: 480, height: 420 },
  },
  [PIN_DIALOG]: { confirmation: true, size: { width: 440, height: 320 } },
};

/** Message queries of the reader (`queryKeys.message` in `state/queries.ts`). */
const MESSAGES = ["message"] as const;

/**
 * Keeps the security views current: keys changed → keyring, recipient
 * status and the shown message's security (trust may have changed);
 * vault progress → the cached status; settings → refetch.
 */
export const handleSecurityEvent: FeatureEventHandler = (event, { queryClient }) => {
  switch (event.type) {
    case "KeysChanged":
      void queryClient.invalidateQueries({ queryKey: securityKeys.all });
      void queryClient.invalidateQueries({ queryKey: MESSAGES });
      break;
    case "VaultProgress":
      applyVaultStatus(queryClient, event.status);
      break;
    case "SecuritySettingsChanged":
      void queryClient.invalidateQueries({ queryKey: securityKeys.settings });
      void queryClient.invalidateQueries({ queryKey: securityKeys.recipients });
      break;
  }
};

/** Handlers for core events (`KeysChanged`, `VaultProgress`, …). */
export const events: Feature["events"] = [handleSecurityEvent, handlePinEvent];

/** Background components mounted once in the app shell. */
export const effects: Feature["effects"] = [RecoveryGate];

// ---------------------------------------------------------------------------
// Mount points for other packages (see ADR 0033, "Mount points").

export { AutocryptImport } from "./Autocrypt";
export { ComposerSecurity, composerRecipients } from "./ComposerSecurity";
export {
  composerCrypto,
  composerCryptoStore,
  composerSendBlock,
  composerSendCrypto,
  loadComposerCrypto,
} from "./composerStore";
export { IdentityCryptoFields } from "./IdentityCrypto";
export { AuthBadge, MessageBadges, SecurityBadge, SecurityDetails } from "./MessageSecurity";
export { attachmentWarning, warningsForLink } from "./model";
export { AttachmentWarningMark, MessageWarnings } from "./Warnings";
export { pinStoreFor, PIN_DIALOG } from "./PinPrompt";
export { formatRecoveryKey, parseRecoveryKey } from "./recoveryKeyText";

/**
 * Settings labels per widget kind, for the settings search (M5b WP163, ADR
 * 0109): Fluent ids of the setting labels each widget shows.
 */
export const settings: NonNullable<Feature["settings"]> = {
  "security-keys": {
    labels: [
      "security-settings-tab-keys",
      "security-keys-label",
      "security-keys-list",
      "security-keys-tools",
      "security-keys-own-only",
    ],
    keywords: "settings-keywords-security-keys",
  },
  "security-identities": {
    labels: [
      "security-settings-tab-identities",
      "security-identities-label",
      "security-identity-sign",
      "security-identity-encrypt",
      "security-identity-autocrypt",
      "security-identity-prefer-encrypt",
      "security-identity-protect-headers",
      "security-identity-openpgp-key",
      "security-identity-smime-cert",
      "security-identity-technology",
    ],
    keywords: "settings-keywords-security-keys",
  },
  "security-settings": {
    labels: [
      "security-settings-tab-protection",
      "security-settings-label",
      "security-vault-title",
      "security-vault-turn-on",
      "security-index-title",
      "security-lookup-title",
      "security-lookup-policy",
    ],
    keywords: "settings-keywords-security-settings",
  },
  "security-autocrypt": {
    labels: [
      "security-settings-tab-autocrypt",
      "security-autocrypt-label",
      "security-autocrypt-send-title",
      "security-autocrypt-import-title",
    ],
  },
  [RECOVERY_WIDGET]: {
    labels: [
      "security-settings-tab-recovery-key",
      "security-recovery-key-label",
      "security-recovery-title",
      "security-recovery-create",
      "security-recovery-check",
      "security-recovery-replace",
      "security-recovery-remove",
    ],
  },
  [SMARTCARDS_WIDGET]: {
    labels: [
      "security-settings-tab-smartcards",
      "security-smartcard-title",
      "security-smartcard-modules",
      "security-smartcard-add-module",
      "security-smartcard-cards",
      "security-smartcard-pin-cache",
      "security-smartcard-use-openpgp",
      "security-smartcard-use-smime",
    ],
    keywords: "settings-keywords-security-smartcards",
  },
};
