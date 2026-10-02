/**
 * The mock core's answers to the M4c recovery-key and smartcard requests
 * (ADR 0094), used by `MockTransport` (`../../api/mock.ts` routes exactly
 * these requests here).
 *
 * It simulates what `nail-vault`, `nail-pkcs11` and the core do (WP122 /
 * WP125):
 * - the recovery key: created only for an encrypted vault (else `conflict`
 *   + `vault-not-encrypted`), shown once, checked (`recovery-key-issue-*`
 *   for typos, `auth_failed` + `recovery-key-wrong` for another key),
 *   restoring the keychain entry when `needs_recovery`, removed; the status
 *   lives in the security mock's `VaultStatus`;
 * - smartcards: the settings (stored and checked like the core), tokens of
 *   the enabled modules (a module whose path contains "missing" reports
 *   `module-not-found`), using a key (a keyring entry whose secret stays on
 *   the card), and PIN prompts: `simulatePinRequest` sends `PinRequested`;
 *   a wrong PIN asks again with `retry` and one attempt less, the right one
 *   (`correctPin`) or a cancel ends it; `closePinPrompt` is the core
 *   giving up (timeout, card removed: `PinPromptClosed`).
 */
import type {
  CryptoTechnology,
  PinPrompt,
  RecoveryKeyStatus,
  Request,
  Response,
  SmartcardKeyRef,
  SmartcardSettings,
  SmartcardToken,
  VaultStatus,
} from "../../api/generated";
import type { MockCore } from "../../api/mockMail";
import { makeKey } from "./mockSeed";
import { formatRecoveryKey, normalizeRecoveryKey, parseRecoveryKey } from "./recoveryKeyText";
import { PIN_PROMPT_TIMEOUT_SECS, smartcardSettingsIssue } from "./smartcardModel";

export { smartcardSettingsIssue } from "./smartcardModel";

/** The requests this file answers. */
export const VAULT_REQUESTS = [
  "CreateRecoveryKey",
  "VerifyRecoveryKey",
  "RestoreVaultKey",
  "RemoveRecoveryKey",
  "GetSmartcardSettings",
  "SetSmartcardSettings",
  "ListSmartcards",
  "UseSmartcardKey",
  "AnswerPinPrompt",
] as const satisfies readonly Request["type"][];

export type VaultRequest = Extract<Request, { type: (typeof VAULT_REQUESTS)[number] }>;

export function isVaultRequest(request: Request): request is VaultRequest {
  return (VAULT_REQUESTS as readonly string[]).includes(request.type);
}

/** Mock-core state (kept on `data.features.vault`). */
export interface VaultMockState {
  smartcards: SmartcardSettings;
  /** The current recovery key (formatted), `null` = none. */
  recoveryKey: string | null;
  /** Recovery-kit files written. */
  kits: string[];
  /** Tokens per module path (a module without an entry has the demo card). */
  tokens: Record<string, SmartcardToken[]>;
  /** The PIN the simulated cards accept. */
  correctPin: string;
  /** Open PIN prompts by id. */
  prompts: Record<string, PinPrompt>;
  /** PINs answered (tests check they are never kept elsewhere). */
  answered: number;
  /**
   * The data directory itself is locked (full-file encryption, WP125):
   * restoring the key needs a restart, as the core says.
   */
  restartAfterRestore: boolean;
}

export function createVaultMockState(): VaultMockState {
  return {
    smartcards: { modules: [], pin_cache_minutes: 0 },
    recoveryKey: null,
    kits: [],
    tokens: {},
    correctPin: "123456",
    prompts: {},
    answered: 0,
    restartAfterRestore: false,
  };
}

/** The demo card of a module (an OpenPGP card with an S/MIME certificate). */
export function demoToken(module: string): SmartcardToken {
  return {
    module,
    slot_id: 1,
    label: "Ada's YubiKey",
    manufacturer: "Yubico",
    model: "YubiKey 5 NFC",
    serial: "15873402",
    login_required: true,
    pin_count_low: false,
    pin_locked: false,
    objects: [
      {
        kind: "private-key",
        id: "01",
        label: "PIV Authentication / Signature",
        algorithm: "ECDSA P-256",
        usages: ["sign", "decrypt"],
        certificate: {
          subject: "CN=Ada Lovelace,E=ada@example.org",
          issuer: "CN=Demo Mail CA,O=Demo Trust",
          serial: "7A21C4",
          not_before: "2026-01-01T00:00:00Z",
          not_after: "2028-01-01T00:00:00Z",
          emails: ["ada@example.org"],
          sha256: "3F9A".repeat(16),
        },
        openpgp_fingerprint: null,
        key_id: null,
      },
      {
        kind: "certificate",
        id: "01",
        label: "Certificate for PIV Authentication",
        algorithm: null,
        usages: [],
        certificate: {
          subject: "CN=Ada Lovelace,E=ada@example.org",
          issuer: "CN=Demo Mail CA,O=Demo Trust",
          serial: "7A21C4",
          not_before: "2026-01-01T00:00:00Z",
          not_after: "2028-01-01T00:00:00Z",
          emails: ["ada@example.org"],
          sha256: "3F9A".repeat(16),
        },
        openpgp_fingerprint: null,
        key_id: null,
      },
      {
        kind: "private-key",
        id: "02",
        label: "OpenPGP signature key",
        algorithm: "Ed25519",
        usages: ["sign"],
        certificate: null,
        openpgp_fingerprint: "8C1F2E0B7D4A9C3E5F6A7B8C9D0E1F2A3B4C5D6E",
        key_id: null,
      },
    ],
  };
}

function vaultOf(core: MockCore): VaultStatus {
  return core.data.features.security.vault;
}

function now(): string {
  return new Date().toISOString();
}

function error(
  code: Extract<Response, { type: "Error" }>["code"],
  id: string,
  message = id,
): Response {
  return { type: "Error", code, message, localized: { id } };
}

function randomKey(): Uint8Array {
  const bytes = new Uint8Array(32);
  crypto.getRandomValues(bytes);
  return bytes;
}

function setRecovery(core: MockCore, recovery: RecoveryKeyStatus): void {
  vaultOf(core).recovery = recovery;
  core.emit({ type: "VaultProgress", status: structuredClone(vaultOf(core)) });
}

/** Tokens of the enabled modules, and the modules that failed. */
function listTokens(state: VaultMockState) {
  const tokens: SmartcardToken[] = [];
  const problems: Extract<Response, { type: "Smartcards" }>["problems"] = [];
  for (const module of state.smartcards.modules.filter((m) => m.enabled)) {
    if (module.path.toLowerCase().includes("missing")) {
      problems.push({
        module: module.path,
        error: "module-not-found",
        detail: `LoadLibraryExW failed for ${module.path}`,
      });
      continue;
    }
    tokens.push(...(state.tokens[module.path] ??= [demoToken(module.path)]));
  }
  return { tokens: structuredClone(tokens), problems };
}

function addCardKey(core: MockCore, ref: SmartcardKeyRef, technology: CryptoTechnology): Response {
  const state = core.data.features.vault;
  const token = (state.tokens[ref.module] ?? []).find((t) => t.serial === ref.token_serial);
  const object = token?.objects.find((o) => o.kind === "private-key" && o.id === ref.object_id);
  if (!token || !object) return error("not_found", "smartcard-error-key-not-found");
  if (object.key_id) return error("conflict", "security-smartcard-in-use");
  const security = core.data.features.security;
  const id = `smartcard-${ref.token_serial}-${ref.object_id}-${technology}`;
  const userIds = object.certificate
    ? [object.certificate.subject]
    : [`${token.label} <ada@example.org>`];
  const key = makeKey(id, technology, userIds, {
    has_secret: true,
    secret_storage: "smartcard",
    source: "smartcard",
    trust: "verified",
    algorithm: object.algorithm ?? "RSA 2048",
    can_encrypt: object.usages.includes("decrypt"),
    can_sign: object.usages.includes("sign"),
    smartcard: structuredClone(ref),
    ...(object.openpgp_fingerprint ? { fingerprint: object.openpgp_fingerprint } : {}),
    ...(object.certificate ? { fingerprint: object.certificate.sha256 } : {}),
  });
  security.keys.push({ key, passphrase: null });
  object.key_id = id;
  for (const o of token.objects) if (o.id === object.id) o.key_id = id;
  core.emit({ type: "KeysChanged" });
  return { type: "Key", key: structuredClone(key) };
}

/**
 * Like a backend needing the PIN: sends `PinRequested` (the first token of
 * the enabled modules, or the demo card) and returns the prompt.
 */
export function simulatePinRequest(core: MockCore, overrides: Partial<PinPrompt> = {}): PinPrompt {
  const state = core.data.features.vault;
  const token = listTokens(state).tokens[0] ?? demoToken("C:\\Windows\\System32\\demo-pkcs11.dll");
  const prompt: PinPrompt = {
    prompt_id: core.nextId("pin-"),
    token_label: token.label,
    token_serial: token.serial,
    purpose: "sign",
    key_label: token.objects[0]?.label ?? null,
    attempts_left: 3,
    retry: false,
    expires_at: new Date(Date.now() + PIN_PROMPT_TIMEOUT_SECS * 1000).toISOString(),
    ...overrides,
  };
  state.prompts[prompt.prompt_id] = prompt;
  core.emit({ type: "PinRequested", prompt: structuredClone(prompt) });
  return prompt;
}

function forgetPrompt(state: VaultMockState, promptId: string): void {
  state.prompts = Object.fromEntries(
    Object.entries(state.prompts).filter(([id]) => id !== promptId),
  );
}

/** Like the core when a card is removed or the prompt times out. */
export function closePinPrompt(
  core: MockCore,
  promptId: string,
  reason: "timed-out" | "token-removed" | "cancelled" | "answered",
): void {
  forgetPrompt(core.data.features.vault, promptId);
  core.emit({ type: "PinPromptClosed", prompt_id: promptId, reason });
}

function answerPin(core: MockCore, promptId: string, pin: string | null): Response {
  const state = core.data.features.vault;
  const prompt = state.prompts[promptId];
  if (!prompt) return error("not_found", "security-pin-prompt-gone");
  state.answered++;
  forgetPrompt(state, promptId);
  // The core tells other windows (`PinPromptClosed` "answered"); the mock
  // has only the answering one.
  if (pin === null) return { type: "Done" };
  if (pin !== state.correctPin) {
    const left = (prompt.attempts_left ?? 3) - 1;
    if (left > 0) {
      core.later(() => {
        simulatePinRequest(core, { ...prompt, retry: true, attempts_left: left });
      });
    }
  }
  return { type: "Done" };
}

export function answerVault(core: MockCore, request: VaultRequest): Response {
  const state = core.data.features.vault;
  switch (request.type) {
    case "GetSmartcardSettings":
      return { type: "SmartcardSettings", settings: structuredClone(state.smartcards) };
    case "SetSmartcardSettings": {
      const issue = smartcardSettingsIssue(request.settings);
      if (issue !== null) {
        return { type: "Error", code: "invalid_request", message: issue, localized: { id: issue } };
      }
      state.smartcards = structuredClone(request.settings);
      core.emit({ type: "SmartcardsChanged" });
      return { type: "SmartcardSettings", settings: structuredClone(state.smartcards) };
    }
    case "ListSmartcards":
      return { type: "Smartcards", ...listTokens(state) };
    case "UseSmartcardKey":
      return addCardKey(core, request.key, request.technology);
    case "AnswerPinPrompt":
      return answerPin(core, request.prompt_id, request.pin);
    case "CreateRecoveryKey": {
      if (vaultOf(core).state !== "encrypted") return error("conflict", "vault-not-encrypted");
      const key = formatRecoveryKey(randomKey());
      state.recoveryKey = key;
      if (request.save_to !== null) state.kits.push(request.save_to);
      const status: RecoveryKeyStatus = { configured: true, created_at: now(), verified_at: null };
      setRecovery(core, status);
      return { type: "RecoveryKeyCreated", key, saved_to: request.save_to, status };
    }
    case "VerifyRecoveryKey": {
      const parsed = parseRecoveryKey(request.key);
      if (!parsed.ok) {
        return error("invalid_request", `recovery-key-issue-${parsed.issue}`);
      }
      const valid =
        state.recoveryKey !== null && normalizeRecoveryKey(request.key) === state.recoveryKey;
      if (valid) setRecovery(core, { ...vaultOf(core).recovery, verified_at: now() });
      return {
        type: "RecoveryKeyVerified",
        valid,
        status: structuredClone(vaultOf(core).recovery),
      };
    }
    case "RestoreVaultKey": {
      const parsed = parseRecoveryKey(request.key);
      if (!parsed.ok) {
        return error("invalid_request", `recovery-key-issue-${parsed.issue}`);
      }
      if (state.recoveryKey === null || normalizeRecoveryKey(request.key) !== state.recoveryKey) {
        return error("auth_failed", "recovery-key-wrong");
      }
      const vault = vaultOf(core);
      vault.needs_recovery = false;
      vault.key_in_keychain = true;
      vault.recovery = { ...vault.recovery, verified_at: now() };
      if (state.restartAfterRestore) {
        vault.error = null;
        vault.localized = { id: "vault-restart-needed", args: {} };
      }
      core.emit({ type: "VaultProgress", status: structuredClone(vault) });
      return { type: "VaultStatus", status: structuredClone(vault) };
    }
    case "RemoveRecoveryKey": {
      state.recoveryKey = null;
      setRecovery(core, { configured: false, created_at: null, verified_at: null });
      return { type: "VaultStatus", status: structuredClone(vaultOf(core)) };
    }
  }
}

/**
 * Puts the mock vault into "encrypted, key lost" with a known recovery key
 * (tests and demos of the restore flow); returns the key.
 */
export function simulateLostVaultKey(core: MockCore): string {
  const state = core.data.features.vault;
  const key = state.recoveryKey ?? formatRecoveryKey(randomKey());
  state.recoveryKey = key;
  const vault = vaultOf(core);
  vault.state = "encrypted";
  vault.key_in_keychain = false;
  vault.needs_recovery = true;
  vault.recovery = {
    configured: true,
    created_at: vault.recovery.created_at ?? "2026-09-01T10:00:00Z",
    verified_at: vault.recovery.verified_at,
  };
  core.emit({ type: "VaultProgress", status: structuredClone(vault) });
  return key;
}
