import { describe, expect, it } from "vitest";
import type { Event as NailEvent } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import {
  formatRecoveryKey,
  normalizeRecoveryKey,
  parseRecoveryKey,
  crc32,
} from "./recoveryKeyText";
import { pinIsPlausible } from "./smartcardModel";
import {
  closePinPrompt,
  isVaultRequest,
  simulateLostVaultKey,
  simulatePinRequest,
  smartcardSettingsIssue,
} from "./vaultMock";

const MODULE = "C:\\Windows\\System32\\opensc-pkcs11.dll";

function transport() {
  const t = new MockTransport();
  const events: NailEvent[] = [];
  t.onEvent((e) => events.push(e));
  return { t, events };
}

describe("recovery key text form (nail_api::recovery)", () => {
  it("matches the standard CRC-32 and the core's format", () => {
    expect(crc32(new TextEncoder().encode("123456789"))).toBe(0xcbf43926);
    // 32 zero bytes: 52 zeros, then the top 20 bits of 0x190A55AD.
    const zero = formatRecoveryKey(new Uint8Array(32));
    expect(zero).toBe(`${"0000-".repeat(13)}3455`);
    expect(zero.split("-")).toHaveLength(14);
  });

  it("round-trips and forgives case, spaces and lookalikes", () => {
    const bytes = Uint8Array.from({ length: 32 }, (_, i) => (i * 37 + 11) & 0xff);
    const text = formatRecoveryKey(bytes);
    const parsed = parseRecoveryKey(text.toLowerCase().replaceAll("-", " "));
    expect(parsed.ok && [...parsed.bytes]).toEqual([...bytes]);
    expect(normalizeRecoveryKey(formatRecoveryKey(new Uint8Array(32)).replaceAll("0", "O"))).toBe(
      formatRecoveryKey(new Uint8Array(32)),
    );
  });

  it("catches mistakes", () => {
    const text = formatRecoveryKey(Uint8Array.from({ length: 32 }, (_, i) => i));
    expect(parseRecoveryKey("  ")).toEqual({ ok: false, issue: "empty" });
    expect(parseRecoveryKey("ABCD-U000")).toEqual({ ok: false, issue: "invalid-character" });
    expect(parseRecoveryKey(text.slice(0, 20))).toEqual({ ok: false, issue: "wrong-length" });
    const typo = (text[0] === "A" ? "B" : "A") + text.slice(1);
    expect(parseRecoveryKey(typo)).toEqual({ ok: false, issue: "checksum-mismatch" });
  });
});

describe("recovery key and smartcard mock", () => {
  it("routes its requests", () => {
    expect(isVaultRequest({ type: "ListSmartcards" })).toBe(true);
    expect(isVaultRequest({ type: "GetVaultStatus" })).toBe(false);
  });

  it("validates smartcard settings and PINs like the core", () => {
    const module = (path: string) => ({ path, name: null, enabled: true });
    expect(smartcardSettingsIssue({ modules: [], pin_cache_minutes: 0 })).toBeNull();
    expect(smartcardSettingsIssue({ modules: [module("opensc.dll")], pin_cache_minutes: 0 })).toBe(
      "smartcard-settings-issue-relative-path",
    );
    expect(
      smartcardSettingsIssue({
        modules: [module("C:\\a\\x.dll"), module("c:\\A\\X.dll")],
        pin_cache_minutes: 0,
      }),
    ).toBe("smartcard-settings-issue-duplicate-module");
    expect(smartcardSettingsIssue({ modules: [], pin_cache_minutes: 481 })).toBe(
      "smartcard-settings-issue-cache-too-long",
    );
    expect(pinIsPlausible("123")).toBe(false);
    expect(pinIsPlausible("123456")).toBe(true);
    expect(pinIsPlausible("12\u000734")).toBe(false);
  });

  it("creates a recovery key only for an encrypted vault and checks it", async () => {
    const { t } = transport();
    expect(await t.send({ type: "CreateRecoveryKey", save_to: null })).toMatchObject({
      code: "conflict",
      localized: { id: "vault-not-encrypted" },
    });
    t.data.features.security.vault.state = "encrypted";
    const created = await t.send({ type: "CreateRecoveryKey", save_to: "C:\\kit.txt" });
    if (created.type !== "RecoveryKeyCreated") throw new Error(created.type);
    expect(parseRecoveryKey(created.key).ok).toBe(true);
    expect(created.status.configured).toBe(true);
    expect(t.data.features.vault.kits).toEqual(["C:\\kit.txt"]);
    expect(await t.send({ type: "VerifyRecoveryKey", key: "nonsense!" })).toMatchObject({
      code: "invalid_request",
      localized: { id: "recovery-key-issue-invalid-character" },
    });
    const other = formatRecoveryKey(new Uint8Array(32));
    expect(await t.send({ type: "VerifyRecoveryKey", key: other })).toMatchObject({
      type: "RecoveryKeyVerified",
      valid: false,
    });
    const verified = await t.send({ type: "VerifyRecoveryKey", key: created.key.toLowerCase() });
    expect(verified).toMatchObject({ valid: true, status: { configured: true } });
    expect(t.data.features.security.vault.recovery.verified_at).not.toBeNull();
    expect(await t.send({ type: "RemoveRecoveryKey" })).toMatchObject({
      type: "VaultStatus",
      status: { recovery: { configured: false } },
    });
  });

  it("restores a lost vault key with the recovery key", async () => {
    const { t, events } = transport();
    const key = simulateLostVaultKey(t);
    expect(events.at(-1)).toMatchObject({
      type: "VaultProgress",
      status: { needs_recovery: true },
    });
    expect(
      await t.send({ type: "RestoreVaultKey", key: formatRecoveryKey(new Uint8Array(32)) }),
    ).toMatchObject({ code: "auth_failed", localized: { id: "recovery-key-wrong" } });
    expect(await t.send({ type: "RestoreVaultKey", key })).toMatchObject({
      type: "VaultStatus",
      status: { needs_recovery: false, key_in_keychain: true },
    });
  });

  it("lists tokens of enabled modules, reports broken ones and uses keys", async () => {
    const { t, events } = transport();
    const settings = {
      modules: [
        { path: MODULE, name: null, enabled: true },
        { path: "C:\\missing\\vendor.dll", name: "Vendor", enabled: true },
      ],
      pin_cache_minutes: 5,
    };
    expect(await t.send({ type: "SetSmartcardSettings", settings })).toEqual({
      type: "SmartcardSettings",
      settings,
    });
    expect(events.some((e) => e.type === "SmartcardsChanged")).toBe(true);
    const listed = await t.send({ type: "ListSmartcards" });
    if (listed.type !== "Smartcards") throw new Error(listed.type);
    expect(listed.problems).toMatchObject([{ error: "module-not-found" }]);
    const token = listed.tokens[0];
    expect(token?.objects.length).toBeGreaterThan(0);
    const ref = { module: MODULE, token_serial: token?.serial ?? "", object_id: "01" };
    const used = await t.send({ type: "UseSmartcardKey", key: ref, technology: "smime" });
    expect(used).toMatchObject({
      type: "Key",
      key: { secret_storage: "smartcard", source: "smartcard", smartcard: ref },
    });
    expect(events.some((e) => e.type === "KeysChanged")).toBe(true);
    expect(await t.send({ type: "UseSmartcardKey", key: ref, technology: "smime" })).toMatchObject({
      code: "conflict",
    });
    expect(
      await t.send({
        type: "UseSmartcardKey",
        key: { ...ref, object_id: "ff" },
        technology: "openpgp",
      }),
    ).toMatchObject({ code: "not_found", localized: { id: "smartcard-error-key-not-found" } });
  });

  it("asks again after a wrong PIN and closes prompts", async () => {
    const { t, events } = transport();
    const prompt = simulatePinRequest(t);
    expect(events.at(-1)).toMatchObject({ type: "PinRequested", prompt: { retry: false } });
    expect(
      await t.send({ type: "AnswerPinPrompt", prompt_id: prompt.prompt_id, pin: "000000" }),
    ).toEqual({ type: "Done" });
    await new Promise((resolve) => setTimeout(resolve, 60));
    expect(events.at(-1)).toMatchObject({
      type: "PinRequested",
      prompt: { retry: true, attempts_left: 2 },
    });
    expect(await t.send({ type: "AnswerPinPrompt", prompt_id: "gone", pin: null })).toMatchObject({
      code: "not_found",
    });
    const again = events.at(-1);
    if (again?.type !== "PinRequested") throw new Error("no prompt");
    closePinPrompt(t, again.prompt.prompt_id, "token-removed");
    expect(events.at(-1)).toEqual({
      type: "PinPromptClosed",
      prompt_id: again.prompt.prompt_id,
      reason: "token-removed",
    });
    expect(t.data.features.vault.prompts).toEqual({});
  });
});
