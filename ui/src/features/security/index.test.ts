import type { VaultStatus } from "../../api/generated";
import { createEnv } from "../../test/harness";
import { appRegistry } from "../registry";
import * as security from "./index";
import { securityKeys } from "./queries";

describe("security feature slot", () => {
  it("registers its widgets, action and dialog centrally", () => {
    for (const kind of [
      "security-keys",
      "security-identities",
      "security-settings",
      "security-autocrypt",
      "security-message",
      "security-warnings",
      "security-composer",
    ]) {
      expect(appRegistry.widgets[kind]).toBe(security.widgets[kind]);
    }
    expect(appRegistry.actions["security.autocrypt.import"]).toBeDefined();
    expect(appRegistry.dialogs["security-autocrypt-import"]).toBeDefined();
    expect(appRegistry.events).toContain(security.handleSecurityEvent);
  });

  it("refreshes keys, recipients and messages when keys change", () => {
    const env = createEnv();
    const spy = vi.spyOn(env.queryClient, "invalidateQueries");
    const ctx = { client: env.client, queryClient: env.queryClient, store: env.store };
    security.handleSecurityEvent({ type: "KeysChanged" }, ctx);
    expect(spy).toHaveBeenCalledWith({ queryKey: securityKeys.all });
    expect(spy).toHaveBeenCalledWith({ queryKey: ["message"] });
    spy.mockClear();
    security.handleSecurityEvent({ type: "SecuritySettingsChanged" }, ctx);
    expect(spy).toHaveBeenCalledWith({ queryKey: securityKeys.settings });
    expect(spy).toHaveBeenCalledWith({ queryKey: securityKeys.recipients });
    spy.mockClear();
    security.handleSecurityEvent({ type: "AccountsChanged" }, ctx);
    expect(spy).not.toHaveBeenCalled();
  });

  it("puts vault progress into the cache", () => {
    const env = createEnv();
    const status: VaultStatus = {
      state: "encrypting",
      phase: "blobs",
      done: 1,
      total: 4,
      key_in_keychain: true,
      needs_recovery: false,
      recovery: { configured: false, created_at: null, verified_at: null },
      error: null,
      message: null,
    };
    security.handleSecurityEvent(
      { type: "VaultProgress", status },
      { client: env.client, queryClient: env.queryClient, store: env.store },
    );
    expect(env.queryClient.getQueryData(securityKeys.vault)).toEqual(status);
  });
});
