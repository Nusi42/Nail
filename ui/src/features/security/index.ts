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

/** M4c action ids. */
export const RECOVERY_CREATE_ACTION = "security.recovery-key.create";
export const RECOVERY_CHECK_ACTION = "security.recovery-key.check";
export const RECOVERY_RESTORE_ACTION = "security.recovery-key.restore";

/** Action ids → handlers (prefix `security.`). */
export const actions: Feature["actions"] = {
  /** Imports the selected Autocrypt setup message (asks for the setup code). */
  "security.autocrypt.import": {
    run: (ctx) => ctx.store.getState().openWindow(AUTOCRYPT_IMPORT_DIALOG),
    enabled: (ctx) => ctx.store.getState().selectedMessageId !== null,
  },
  /** Creates (or replaces) the vault recovery key. */
  [RECOVERY_CREATE_ACTION]: {
    run: (ctx) => ctx.store.getState().openWindow(RECOVERY_CREATE_DIALOG),
  },
  /** Checks a stored recovery key. */
  [RECOVERY_CHECK_ACTION]: {
    run: (ctx) => ctx.store.getState().openWindow(RECOVERY_VERIFY_DIALOG),
  },
  /** Puts the data key back with the recovery key. */
  [RECOVERY_RESTORE_ACTION]: {
    run: (ctx) => ctx.store.getState().openWindow(RECOVERY_RESTORE_DIALOG),
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
export {
  closePinPrompt,
  simulateLostVaultKey,
  simulatePinRequest,
  type VaultMockState,
} from "./vaultMock";
