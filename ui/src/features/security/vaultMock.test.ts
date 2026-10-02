import { describe, expect, it } from "vitest";
import { MockTransport } from "../../api/mock";
import { isVaultRequest, smartcardSettingsIssue } from "./vaultMock";

describe("recovery key and smartcard mock (M4c contract)", () => {
  it("routes its requests", () => {
    expect(isVaultRequest({ type: "ListSmartcards" })).toBe(true);
    expect(isVaultRequest({ type: "GetVaultStatus" })).toBe(false);
  });

  it("validates smartcard settings like the core", () => {
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
  });

  it("stores settings and refuses the rest for now", async () => {
    const t = new MockTransport();
    const settings = {
      modules: [{ path: "C:\\Windows\\System32\\opensc-pkcs11.dll", name: null, enabled: true }],
      pin_cache_minutes: 5,
    };
    expect(await t.send({ type: "SetSmartcardSettings", settings })).toEqual({
      type: "SmartcardSettings",
      settings,
    });
    expect(await t.send({ type: "GetSmartcardSettings" })).toEqual({
      type: "SmartcardSettings",
      settings,
    });
    expect(await t.send({ type: "CreateRecoveryKey", save_to: null })).toMatchObject({
      code: "unsupported",
      localized: { id: "m4c-not-ready" },
    });
  });
});
