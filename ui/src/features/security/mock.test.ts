/**
 * The security slot's mock answers like the core after WP30: settings and
 * vault status work, the rest is `unsupported`, and sign / encrypt requests
 * on send are refused rather than sent unprotected.
 */
import { MockTransport } from "../../api/mock";
import type { Event as NailEvent, Request } from "../../api/generated";
import { SECURITY_REQUESTS, isSecurityRequest } from "./mock";

describe("security mock", () => {
  it("serves and updates the security settings", async () => {
    const transport = new MockTransport();
    const events: NailEvent[] = [];
    transport.onEvent((e) => events.push(e));
    const initial = await transport.send({ type: "GetSecuritySettings" });
    expect(initial).toMatchObject({
      type: "SecuritySettings",
      settings: { key_lookup: "on-demand", index_decrypted: [] },
    });
    const account = transport.data.accounts[0]?.id ?? "";
    const updated = await transport.send({
      type: "UpdateSecuritySettings",
      settings: {
        index_decrypted: [account, account],
        key_lookup: "off",
        lookup_sources: ["keys-openpgp-org", "wkd", "keys-openpgp-org"],
      },
    });
    expect(updated).toEqual({
      type: "SecuritySettings",
      settings: {
        index_decrypted: [account],
        key_lookup: "off",
        lookup_sources: ["keys-openpgp-org", "wkd"],
      },
    });
    expect(events).toContainEqual({ type: "SecuritySettingsChanged" });
    const bad = await transport.send({
      type: "UpdateSecuritySettings",
      settings: { index_decrypted: ["nope"], key_lookup: "off", lookup_sources: [] },
    });
    expect(bad).toMatchObject({ type: "Error", code: "invalid_request" });
  });

  it("reports a plain vault and refuses what is not implemented", async () => {
    const transport = new MockTransport();
    expect(await transport.send({ type: "GetVaultStatus" })).toMatchObject({
      type: "VaultStatus",
      status: { state: "plain" },
    });
    const requests: Request[] = [
      { type: "ListKeys", filter: { technology: null, text: null, own_only: false } },
      { type: "SetVaultEncryption", enabled: true },
    ];
    for (const request of requests) {
      expect(isSecurityRequest(request)).toBe(true);
      expect(await transport.send(request)).toMatchObject({ type: "Error", code: "unsupported" });
    }
    expect(new Set(SECURITY_REQUESTS).size).toBe(SECURITY_REQUESTS.length);
  });

  it("refuses to send a signed message instead of sending it plain", async () => {
    const transport = new MockTransport();
    const identity = transport.data.accountsConfig.accounts[0]?.identities[0];
    if (!identity) throw new Error("the mock has no identity");
    const saved = await transport.send({
      type: "SaveDraft",
      draft: {
        id: null,
        identity_id: identity.id,
        to: [{ name: null, email: "bob@example.org" }],
        cc: [],
        bcc: [],
        subject: "Signed",
        mode: "plain",
        text: "hi",
        html: null,
        markdown: null,
        attachments: [],
        context: null,
        signature: "none",
        markdown_part: false,
      },
    });
    if (saved.type !== "DraftSaved") throw new Error(JSON.stringify(saved));
    const outboxBefore = transport.data.outbox.length;
    const refused = await transport.send({
      type: "SendDraft",
      draft_id: saved.draft_id,
      delay_secs: 0,
      send_at: null,
      crypto: { sign: true, encrypt: false, technology: null, protect_headers: null },
    });
    expect(refused).toMatchObject({ type: "Error", code: "unsupported" });
    expect(transport.data.outbox).toHaveLength(outboxBefore);
  });
});
