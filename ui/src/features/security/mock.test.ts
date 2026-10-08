import {
  SECURITY_REQUESTS,
  VAULT_ITEMS,
  armorKey,
  decorateMessageBody,
  isSecurityRequest,
  pemCertificate,
  securityDemoBySender,
} from "./mock";
import { mockSetup } from "../../test/env";

function setup() {
  const core = mockSetup();
  return { ...core, s: core.transport.data.features.security };
}

const ALL = { technology: null, text: null, own_only: false };

describe("security mock", () => {
  it("routes exactly its requests", () => {
    expect(new Set(SECURITY_REQUESTS).size).toBe(SECURITY_REQUESTS.length);
    expect(isSecurityRequest({ type: "ListKeys", filter: ALL })).toBe(true);
    expect(isSecurityRequest({ type: "Ping" })).toBe(false);
  });

  it("lists keys with own keys first and filters", async () => {
    const { send } = setup();
    const all = (await send({ type: "ListKeys", filter: ALL }, "Keys")).keys;
    expect(all.length).toBeGreaterThan(4);
    expect(all.slice(0, 2).every((k) => k.has_secret)).toBe(true);
    const own = (await send({ type: "ListKeys", filter: { ...ALL, own_only: true } }, "Keys")).keys;
    expect(own.map((k) => k.trust)).toEqual(["own", "own"]);
    const smime = (
      await send({ type: "ListKeys", filter: { ...ALL, technology: "smime" } }, "Keys")
    ).keys;
    expect(smime.every((k) => k.technology === "smime" && k.chain !== null)).toBe(true);
    const grace = (await send({ type: "ListKeys", filter: { ...ALL, text: "GRACE" } }, "Keys"))
      .keys;
    expect(grace.map((k) => k.emails[0])).toEqual(["grace@navy.example"]);
    expect(grace[0]?.trust).toBe("verified");
    expect(await send({ type: "GetKey", key_id: "key-3" }, "Key")).toMatchObject({
      key: { id: "key-3" },
    });
    expect(await setup().transport.send({ type: "GetKey", key_id: "nope" })).toMatchObject({
      code: "not_found",
    });
  });

  it("round-trips export and import, merging an existing key", async () => {
    const { send, events } = setup();
    const exported = await send(
      { type: "ExportPublicKey", key_id: "key-3", encoding: "text" },
      "KeyExported",
    );
    expect(exported).toMatchObject({
      filename: "grace@navy.example.asc",
      mime: "application/pgp-keys",
    });
    expect(exported.data).toContain("BEGIN PGP PUBLIC KEY BLOCK");
    const again = await send(
      { type: "ImportKeys", data: exported.data, encoding: "text" },
      "KeysImported",
    );
    expect(again.result).toMatchObject({ created: 0, updated: 1 });
    expect(again.result.keys[0]?.id).toBe("key-3");
    expect(events).toContainEqual({ type: "KeysChanged" });

    const b64 = await send(
      { type: "ExportPublicKey", key_id: "key-5", encoding: "base64" },
      "KeyExported",
    );
    expect(atob(b64.data)).toContain("BEGIN CERTIFICATE");
    expect(b64.filename).toMatch(/\.pem$/);
  });

  it("imports new keys and reports skipped items", async () => {
    const { send } = setup();
    const text = [
      armorKey({
        fingerprint: "1".repeat(40),
        user_ids: ["New <new@x.example>"],
        algorithm: "Ed25519",
      }),
      pemCertificate({ fingerprint: "2".repeat(64), user_ids: ["CN=Cert,E=cert@x.example"] }),
      "-----BEGIN PGP PUBLIC KEY BLOCK-----\n\nAAAA\n-----END PGP PUBLIC KEY BLOCK-----",
    ].join("\n");
    const answer = await send(
      { type: "ImportKeys", data: btoa(text), encoding: "base64" },
      "KeysImported",
    );
    expect(answer.result.created).toBe(2);
    expect(answer.result.issues).toHaveLength(1);
    expect(answer.result.keys.map((k) => k.technology)).toEqual(["openpgp", "smime"]);
    expect(answer.result.keys[0]).toMatchObject({ fingerprint: "1".repeat(40), trust: "unknown" });

    const { transport } = setup();
    expect(
      await transport.send({ type: "ImportKeys", data: "hello", encoding: "text" }),
    ).toMatchObject({ code: "crypto" });
    expect(
      await transport.send({ type: "ImportKeys", data: "%%%", encoding: "base64" }),
    ).toMatchObject({ code: "invalid_request" });
  });

  it("imports PKCS#12 only with the right password", async () => {
    const { send, transport } = setup();
    const data = btoa("certificate for bob@x.example");
    expect(
      await transport.send({
        type: "ImportPkcs12",
        data,
        passphrase: "nope",
        protection: { kind: "keychain" },
      }),
    ).toMatchObject({ code: "auth_failed" });
    const ok = await send(
      { type: "ImportPkcs12", data, passphrase: "demo", protection: { kind: "keychain" } },
      "KeysImported",
    );
    expect(ok.result.keys[0]).toMatchObject({
      technology: "smime",
      has_secret: true,
      source: "pkcs12",
      trust: "own",
      emails: ["bob@x.example"],
    });
  });

  it("generates keys for identities", async () => {
    const { send, transport } = setup();
    const key = (
      await send(
        {
          type: "GenerateKey",
          identity_id: "ada@work.example",
          algorithm: "rsa4096",
          expires_days: 365,
          protection: { kind: "passphrase", passphrase: "pw" },
        },
        "KeyGenerated",
      )
    ).key;
    expect(key).toMatchObject({
      algorithm: "RSA 4096",
      has_secret: true,
      secret_storage: "passphrase",
      emails: ["ada@work.example"],
      source: "generated",
    });
    expect(key.expires_at).not.toBeNull();
    expect(
      await transport.send({
        type: "UnlockKey",
        key_id: key.id,
        passphrase: "bad",
        remember_minutes: null,
      }),
    ).toMatchObject({ code: "auth_failed" });
    expect(
      await transport.send({
        type: "UnlockKey",
        key_id: key.id,
        passphrase: "pw",
        remember_minutes: 5,
      }),
    ).toEqual({ type: "Done" });
    expect(
      await transport.send({
        type: "GenerateKey",
        identity_id: "who",
        algorithm: "curve25519",
        expires_days: null,
        protection: { kind: "keychain" },
      }),
    ).toMatchObject({ code: "not_found" });
  });

  it("keeps trust decisions across delete and re-import", async () => {
    const { send, transport } = setup();
    const saved = await send(
      { type: "SetKeyTrust", key_id: "key-4", decision: "rejected" },
      "KeySaved",
    );
    expect(saved.key.trust).toBe("rejected");
    const exported = await send(
      { type: "ExportPublicKey", key_id: "key-4", encoding: "text" },
      "KeyExported",
    );
    expect(
      await transport.send({ type: "DeleteKey", key_id: "key-4", delete_secret: false }),
    ).toEqual({ type: "Done" });
    const back = await send(
      { type: "ImportKeys", data: exported.data, encoding: "text" },
      "KeysImported",
    );
    expect(back.result.keys[0]?.trust).toBe("rejected");
    const unset = await send(
      { type: "SetKeyTrust", key_id: back.result.keys[0]?.id ?? "", decision: "unset" },
      "KeySaved",
    );
    expect(unset.key.trust).toBe("unknown");
  });

  it("refuses to delete an own key without delete_secret", async () => {
    const { transport } = setup();
    expect(
      await transport.send({ type: "DeleteKey", key_id: "key-1", delete_secret: false }),
    ).toMatchObject({ code: "conflict" });
    expect(
      await transport.send({ type: "DeleteKey", key_id: "key-1", delete_secret: true }),
    ).toEqual({ type: "Done" });
  });

  it("looks up keys without importing them", async () => {
    const { send, transport, s } = setup();
    const found = await send(
      { type: "LookupKeys", email: "Liskov@MIT.example", sources: ["wkd", "keys-openpgp-org"] },
      "KeyLookup",
    );
    expect(found.email).toBe("liskov@mit.example");
    expect(found.candidates).toHaveLength(1);
    expect(found.candidates[0]).toMatchObject({ source: "wkd", known: null });
    const before = s.keys.length;
    const onlyKeyserver = await send(
      { type: "LookupKeys", email: "liskov@mit.example", sources: ["keys-openpgp-org"] },
      "KeyLookup",
    );
    expect(onlyKeyserver.candidates).toEqual([]);
    expect(s.keys).toHaveLength(before);
    await send(
      { type: "ImportKeys", data: found.candidates[0]?.armored ?? "", encoding: "text" },
      "KeysImported",
    );
    const known = await send(
      { type: "LookupKeys", email: "liskov@mit.example", sources: ["wkd"] },
      "KeyLookup",
    );
    expect(known.candidates[0]?.known).not.toBeNull();

    const failed = await send(
      { type: "LookupKeys", email: "a@unreachable.example", sources: ["wkd"] },
      "KeyLookup",
    );
    expect(failed.failures).toEqual([
      { source: "wkd", code: "network", message: "connection refused" },
    ]);

    s.settings.key_lookup = "off";
    expect(
      await transport.send({ type: "LookupKeys", email: "liskov@mit.example", sources: ["wkd"] }),
    ).toMatchObject({ code: "unsupported" });
  });

  it("lists and imports Windows store certificates", async () => {
    const { send, transport, s } = setup();
    const personal = (
      await send({ type: "ListWindowsCertificates", store: "personal" }, "WindowsCertificates")
    ).certificates;
    expect(personal).toHaveLength(2);
    const usable = personal.find((c) => c.usable_for_email);
    const unusable = personal.find((c) => !c.usable_for_email);
    expect(
      await transport.send({
        type: "ImportWindowsCertificate",
        store: "personal",
        thumbprint: unusable?.thumbprint ?? "",
      }),
    ).toMatchObject({ code: "invalid_request" });
    const imported = await send(
      { type: "ImportWindowsCertificate", store: "personal", thumbprint: usable?.thumbprint ?? "" },
      "KeysImported",
    );
    expect(imported.result.keys[0]).toMatchObject({
      secret_storage: "windows-store",
      has_secret: true,
      trust: "own",
    });
    const again = (
      await send({ type: "ListWindowsCertificates", store: "personal" }, "WindowsCertificates")
    ).certificates;
    expect(again.find((c) => c.usable_for_email)?.imported).toBe(imported.result.keys[0]?.id);
    s.windows = false;
    expect(
      await transport.send({ type: "ListWindowsCertificates", store: "personal" }),
    ).toMatchObject({ code: "unsupported" });
  });

  it("reports recipient keys, Autocrypt and a suggestion", async () => {
    const { send, transport, s } = setup();
    const report = (
      await send(
        {
          type: "CheckRecipientKeys",
          identity_id: "ada@example.org",
          recipients: [
            "Grace@navy.example",
            "grace@navy.example",
            "ewd@tue.example",
            "liskov@mit.example",
          ],
          lookup: false,
        },
        "RecipientKeys",
      )
    ).report;
    expect(report.recipients.map((r) => r.email)).toEqual([
      "grace@navy.example",
      "ewd@tue.example",
      "liskov@mit.example",
    ]);
    expect(report.recipients.map((r) => r.openpgp?.state ?? null)).toEqual([
      "usable",
      "expired",
      null,
    ]);
    expect(report.recipients.every((r) => r.lookup === "not-attempted")).toBe(true);
    expect(report.openpgp).toEqual({ own_key: true, all_recipients: false });
    expect(report.smime.own_key).toBe(false);
    expect(report.suggested).toMatchObject({ sign: false, encrypt: false });

    const looked = (
      await send(
        {
          type: "CheckRecipientKeys",
          identity_id: "ada@example.org",
          recipients: [
            "ewd@tue.example",
            "liskov@mit.example",
            "x@unreachable.example",
            "nobody@x.example",
          ],
          lookup: true,
        },
        "RecipientKeys",
      )
    ).report;
    expect(looked.recipients.map((r) => r.lookup)).toEqual([
      "found",
      "found",
      "failed",
      "not-found",
    ]);

    // Autocrypt: Alan prefers encryption; with prefer-encrypt mutual the core suggests encrypting.
    const identity = transport.data.accountsConfig.accounts[0]?.identities[0];
    if (!identity) throw new Error("no identity");
    identity.crypto = {
      technology: "auto",
      sign: true,
      encrypt: false,
      openpgp_key: null,
      smime_certificate: null,
      autocrypt: true,
      prefer_encrypt: "mutual",
      protect_headers: true,
    };
    const alan = (
      await send(
        {
          type: "CheckRecipientKeys",
          identity_id: identity.id,
          recipients: ["alan@bletchley.example"],
          lookup: false,
        },
        "RecipientKeys",
      )
    ).report;
    expect(alan.autocrypt).toBe("encrypt");
    expect(alan.recipients[0]?.autocrypt?.prefer_encrypt).toBe("mutual");
    expect(alan.suggested).toMatchObject({ sign: true, encrypt: true });

    // Grace is known but not via Autocrypt.
    const mixed = (
      await send(
        {
          type: "CheckRecipientKeys",
          identity_id: identity.id,
          recipients: ["alan@bletchley.example", "grace@navy.example"],
          lookup: false,
        },
        "RecipientKeys",
      )
    ).report;
    expect(mixed.autocrypt).toBe("available");
    s.peers["grace@navy.example"] = {
      email: "grace@navy.example",
      last_seen: "2026-01-01T00:00:00Z",
      autocrypt_at: null,
      prefer_encrypt: "nopreference",
      key_fingerprint: null,
      gossip_fingerprint: "AA",
    };
    const gossip = (
      await send(
        {
          type: "CheckRecipientKeys",
          identity_id: identity.id,
          recipients: ["alan@bletchley.example", "grace@navy.example"],
          lookup: false,
        },
        "RecipientKeys",
      )
    ).report;
    expect(gossip.autocrypt).toBe("discourage");

    // S/MIME for Katherine from the work identity.
    const work = (
      await send(
        {
          type: "CheckRecipientKeys",
          identity_id: "ada@work.example",
          recipients: ["katherine@nasa.example"],
          lookup: false,
        },
        "RecipientKeys",
      )
    ).report;
    expect(work.smime).toEqual({ own_key: true, all_recipients: true });
    expect(work.openpgp.own_key).toBe(false);

    expect(
      await transport.send({
        type: "CheckRecipientKeys",
        identity_id: "who",
        recipients: [],
        lookup: false,
      }),
    ).toMatchObject({ code: "not_found" });
  });

  it("sends an Autocrypt setup message and imports one with its code", async () => {
    const { send, transport, s } = setup();
    const sent = await send(
      { type: "SendAutocryptSetup", identity_id: "ada@example.org" },
      "AutocryptSetupSent",
    );
    expect(sent.passcode).toMatch(/^(\d{4}-){8}\d{4}$/);
    expect(sent.outbox_id).toMatch(/^out-autocrypt-/);
    expect(
      await transport.send({ type: "SendAutocryptSetup", identity_id: "ada@work.example" }),
    ).toMatchObject({ code: "no_key" });

    const messageId = [...transport.data.bodies.keys()][0] ?? "";
    expect(
      await transport.send({ type: "ImportAutocryptSetup", message_id: messageId, passcode: "1" }),
    ).toMatchObject({ code: "invalid_request" });
    s.setupInbox[messageId] = sent.passcode;
    expect(
      await transport.send({ type: "ImportAutocryptSetup", message_id: messageId, passcode: "1" }),
    ).toMatchObject({ code: "auth_failed" });
    const imported = await send(
      {
        type: "ImportAutocryptSetup",
        message_id: messageId,
        passcode: sent.passcode.replaceAll("-", ""),
      },
      "KeysImported",
    );
    expect(imported.result.keys[0]).toMatchObject({ source: "setup-message", has_secret: true });
  });

  it("migrates the vault with progress events and refuses a second run", async () => {
    vi.useFakeTimers();
    try {
      const { send, transport, events } = setup();
      const started = await send({ type: "SetVaultEncryption", enabled: true }, "VaultStatus");
      expect(started.status).toMatchObject({ state: "encrypting", phase: "database", done: 0 });
      expect(await transport.send({ type: "SetVaultEncryption", enabled: false })).toMatchObject({
        code: "conflict",
      });
      await vi.runAllTimersAsync();
      const progress = events.filter((e) => e.type === "VaultProgress");
      const total = VAULT_ITEMS.database + VAULT_ITEMS.blobs + VAULT_ITEMS.index;
      expect(progress.length).toBeGreaterThanOrEqual(total);
      expect(progress.at(-1)).toMatchObject({
        status: { state: "encrypted", phase: null, key_in_keychain: true },
      });
      const phases = new Set(progress.map((e) => e.status.phase));
      expect([...phases]).toEqual(expect.arrayContaining(["database", "blobs", "index"]));
      // Already encrypted: nothing to do.
      expect(
        await send({ type: "SetVaultEncryption", enabled: true }, "VaultStatus"),
      ).toMatchObject({ status: { state: "encrypted" } });
      await send({ type: "SetVaultEncryption", enabled: false }, "VaultStatus");
      await vi.runAllTimersAsync();
      expect(await send({ type: "GetVaultStatus" }, "VaultStatus")).toMatchObject({
        status: { state: "plain" },
      });
    } finally {
      vi.useRealTimers();
    }
  });

  it("stops a migration with an error that can be retried", async () => {
    vi.useFakeTimers();
    try {
      const { send, s } = setup();
      s.vaultError = "internal";
      await send({ type: "SetVaultEncryption", enabled: true }, "VaultStatus");
      await vi.runAllTimersAsync();
      expect(s.vault).toMatchObject({ state: "encrypting", phase: "blobs", error: "internal" });
      await send({ type: "SetVaultEncryption", enabled: true }, "VaultStatus");
      await vi.runAllTimersAsync();
      expect(s.vault).toMatchObject({ state: "encrypted", error: null });
    } finally {
      vi.useRealTimers();
    }
  });

  it("keeps security settings as the core does", async () => {
    const { send, transport, events } = setup();
    expect(
      await transport.send({
        type: "UpdateSecuritySettings",
        settings: { index_decrypted: ["ghost"], key_lookup: "off", lookup_sources: [] },
      }),
    ).toMatchObject({ code: "invalid_request" });
    const saved = await send(
      {
        type: "UpdateSecuritySettings",
        settings: {
          index_decrypted: ["acc-work", "acc-personal", "acc-work"],
          key_lookup: "automatic",
          lookup_sources: ["wkd", "wkd"],
        },
      },
      "SecuritySettings",
    );
    expect(saved.settings).toEqual({
      index_decrypted: ["acc-personal", "acc-work"],
      key_lookup: "automatic",
      lookup_sources: ["wkd"],
    });
    expect(events).toContainEqual({ type: "SecuritySettingsChanged" });
  });

  it("suggests the authserv-id of the seeded sender checks", async () => {
    const { send, transport, s } = setup();
    expect(
      await send({ type: "SuggestAuthservId", account_id: "acc-work" }, "AuthservIdSuggestion"),
    ).toMatchObject({ authserv_id: null });
    s.bySender = securityDemoBySender();
    expect(
      await send({ type: "SuggestAuthservId", account_id: "acc-work" }, "AuthservIdSuggestion"),
    ).toEqual({
      type: "AuthservIdSuggestion",
      account_id: "acc-work",
      authserv_id: "imap.example.org",
    });
    expect(await transport.send({ type: "SuggestAuthservId", account_id: "ghost" })).toMatchObject({
      code: "not_found",
    });
  });

  it("decorates message bodies by id and by sender", () => {
    const { transport, s } = setup();
    const body = [...transport.data.bodies.values()][0];
    if (!body) throw new Error("no body");
    expect(decorateMessageBody(transport, body)).toBe(body);
    s.bySender = securityDemoBySender();
    const grace = {
      ...body,
      headers: { ...body.headers, from: [{ name: null, email: "Grace@navy.example" }] },
    };
    expect(decorateMessageBody(transport, grace).security?.signatures[0]?.state).toBe("valid");
    s.messages[body.id] = { warnings: [] };
    expect(decorateMessageBody(transport, grace).warnings).toEqual([]);
  });

  it("unlocks messages encrypted to a locked key", async () => {
    const { send, transport, s } = setup();
    s.messages["m-1"] = {
      security: {
        technology: "smime",
        signatures: [],
        encryption: { state: "locked", key_id: "key-2", recipients: [], algorithm: null },
        header_protection: "none",
        partial: false,
        inline_pgp: false,
      },
    };
    expect(
      await transport.send({
        type: "UnlockKey",
        key_id: "key-1",
        passphrase: "x",
        remember_minutes: null,
      }),
    ).toMatchObject({ code: "invalid_request" });
    await send(
      { type: "UnlockKey", key_id: "key-2", passphrase: "demo", remember_minutes: null },
      "Done",
    );
    expect(s.messages["m-1"].security?.encryption?.state).toBe("decrypted");
  });

  it("sends signed or encrypted mail only when the keys allow it, never plain", async () => {
    const { transport } = setup();
    const identity = transport.data.accountsConfig.accounts[0]?.identities[0];
    if (!identity) throw new Error("the mock has no identity");
    const draft = (to: string) => ({
      id: null,
      identity_id: identity.id,
      to: [{ name: null, email: to }],
      cc: [],
      bcc: [],
      subject: "Protected",
      mode: "plain" as const,
      text: "hi",
      html: null,
      markdown: null,
      attachments: [],
      context: null,
      signature: "none" as const,
      markdown_part: false,
    });
    const send = async (to: string, sign: boolean, encrypt: boolean) => {
      const saved = await transport.send({ type: "SaveDraft", draft: draft(to) });
      if (saved.type !== "DraftSaved") throw new Error(JSON.stringify(saved));
      return transport.send({
        type: "SendDraft",
        draft_id: saved.draft_id,
        delay_secs: 60,
        send_at: null,
        crypto: { sign, encrypt, technology: null, protect_headers: null },
      });
    };
    const outboxBefore = transport.data.outbox.length;
    // Nobody has a key for bob@example.org.
    expect(await send("bob@example.org", false, true)).toMatchObject({
      type: "Error",
      code: "no_key",
    });
    expect(transport.data.outbox).toHaveLength(outboxBefore);
    // Grace has a verified key; Ada signs with her own key.
    expect(await send("grace@navy.example", true, true)).toMatchObject({ type: "SendQueued" });
    expect(await send("bob@example.org", true, false)).toMatchObject({ type: "SendQueued" });
    expect(transport.data.outbox).toHaveLength(outboxBefore + 2);
  });
});
