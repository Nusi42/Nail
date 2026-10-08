import {
  MOCK_TOTP_CODE,
  decorateRemoteContent,
  defaultRemoteUiSettings,
  expirePairingCodes,
  findPairingCode,
  mockStatus,
} from "./mock";
import { presetGrant } from "./model";
import { mockSetup } from "../../test/env";

function setup() {
  const core = mockSetup();
  return { ...core, s: core.transport.data.features.remote };
}

async function withUser(ctx: ReturnType<typeof setup>, admin = false) {
  const { user } = await ctx.send(
    {
      type: "CreateRemoteUser",
      name: "ada",
      display_name: "Ada",
      grant: presetGrant("full", { admin }),
    },
    "RemoteUserSaved",
  );
  return user;
}

describe("remote mock (WP43 additions)", () => {
  it("runs while enabled, with addresses and a certificate unless TLS is off", async () => {
    const { send, s } = setup();
    expect(mockStatus(s).state).toBe("stopped");
    const lan = { ...defaultRemoteUiSettings(), enabled: true, bind_address: "0.0.0.0" };
    const on = await send({ type: "UpdateRemoteUiSettings", settings: lan }, "RemoteUi");
    expect(on.status.state).toBe("running");
    expect(on.status.urls).toEqual([
      "https://nail-desktop.local:4780/",
      "https://192.168.1.20:4780/",
    ]);
    expect(on.status.certificate?.fingerprint_sha256).toMatch(/^[0-9A-F]{64}$/);
    expect(on.status.internet_mode_locked).toBe(true);

    const local = { ...lan, bind_address: "127.0.0.1", tls: "off" as const };
    const off = await send({ type: "UpdateRemoteUiSettings", settings: local }, "RemoteUi");
    expect(off.status.urls).toEqual(["http://127.0.0.1:4780/"]);
    expect(off.status.certificate).toBeNull();
  });

  it("regenerates the self-signed certificate with a new fingerprint", async () => {
    const { send, transport, events } = setup();
    const lan = { ...defaultRemoteUiSettings(), enabled: true, bind_address: "0.0.0.0" };
    const before = await send({ type: "UpdateRemoteUiSettings", settings: lan }, "RemoteUi");
    const after = await send({ type: "RegenerateRemoteCertificate" }, "RemoteUi");
    expect(after.status.certificate?.fingerprint_sha256).not.toBe(
      before.status.certificate?.fingerprint_sha256,
    );
    expect(events.filter((e) => e.type === "RemoteUiStatusChanged")).toHaveLength(2);
    await send(
      {
        type: "UpdateRemoteUiSettings",
        settings: { ...lan, bind_address: "127.0.0.1", tls: "off" },
      },
      "RemoteUi",
    );
    expect(await transport.send({ type: "RegenerateRemoteCertificate" })).toMatchObject({
      type: "Error",
      code: "invalid_request",
    });
  });

  it("names the settings issue in the error like the core", async () => {
    const { transport } = setup();
    const bad = await transport.send({
      type: "UpdateRemoteUiSettings",
      settings: { ...defaultRemoteUiSettings(), allowed_networks: ["10.0.0.0/33"] },
    });
    expect(bad).toMatchObject({
      type: "Error",
      code: "invalid_request",
      message: "invalid-network",
    });
    const acme = await transport.send({
      type: "UpdateRemoteUiSettings",
      settings: { ...defaultRemoteUiSettings(), tls: "acme" },
    });
    expect(acme).toMatchObject({ type: "Error", code: "forbidden" });
  });

  it("sets and removes passwords, refusing short ones", async () => {
    const ctx = setup();
    const user = await withUser(ctx);
    const short = await ctx.transport.send({
      type: "SetRemotePassword",
      user_id: user.id,
      password: "short",
    });
    expect(short).toMatchObject({ type: "Error", code: "invalid_request" });
    const set = await ctx.send(
      { type: "SetRemotePassword", user_id: user.id, password: "correct horse battery" },
      "RemoteUserSaved",
    );
    expect(set.user.has_password).toBe(true);
    const cleared = await ctx.send(
      { type: "SetRemotePassword", user_id: user.id, password: null },
      "RemoteUserSaved",
    );
    expect(cleared.user.has_password).toBe(false);
    expect(ctx.s.audit.map((e) => e.kind)).toEqual(["password-changed", "password-changed"]);
  });

  it("enrolls TOTP only with a matching code", async () => {
    const ctx = setup();
    const user = await withUser(ctx);
    const early = await ctx.transport.send({
      type: "ConfirmTotpEnrollment",
      user_id: user.id,
      code: MOCK_TOTP_CODE,
    });
    expect(early).toMatchObject({ type: "Error", code: "invalid_request" });
    const { enrollment } = await ctx.send(
      { type: "StartTotpEnrollment", user_id: user.id },
      "TotpEnrollment",
    );
    expect(enrollment.secret).toMatch(/^[A-Z2-7]{32}$/);
    expect(enrollment.otpauth_uri).toContain(`secret=${enrollment.secret}`);
    expect(enrollment.otpauth_uri).toMatch(/^otpauth:\/\/totp\/Nail%3Aada\?/);
    const wrong = await ctx.transport.send({
      type: "ConfirmTotpEnrollment",
      user_id: user.id,
      code: "000000",
    });
    expect(wrong).toMatchObject({ type: "Error", code: "auth_failed" });
    const ok = await ctx.send(
      { type: "ConfirmTotpEnrollment", user_id: user.id, code: "123 456" },
      "RemoteUserSaved",
    );
    expect(ok.user.totp_enabled).toBe(true);
    const off = await ctx.send({ type: "DisableTotp", user_id: user.id }, "RemoteUserSaved");
    expect(off.user.totp_enabled).toBe(false);
  });

  it("creates single-use pairing codes within the user's grant that expire", async () => {
    const ctx = setup();
    const user = await withUser(ctx, false);
    const created = await ctx.send(
      {
        type: "CreatePairingCode",
        user_id: user.id,
        device_name: " Phone ",
        grant: presetGrant("full", { admin: true }),
        ttl_minutes: 5,
      },
      "PairingCodeCreated",
    );
    expect(created.code).toMatch(/^[A-Z2-9]{4}-[A-Z2-9]{4}$/);
    expect(created.pairing.device_name).toBe("Phone");
    expect(created.pairing.grant.admin).toBe(false);
    expect(created.url).toBeNull();
    expect(ctx.events).toContainEqual({ type: "RemoteAccessChanged" });
    expect(findPairingCode(ctx.s, created.code.toLowerCase())?.id).toBe(created.pairing.id);

    // Expiry.
    expirePairingCodes(ctx.s, Date.now() + 6 * 60_000);
    expect(findPairingCode(ctx.s, created.code)).toBeUndefined();
    const { codes } = await ctx.send({ type: "ListPairingCodes" }, "PairingCodes");
    expect(codes[0]?.state).toBe("expired");

    // Cancelling forgets the code.
    const second = await ctx.send(
      {
        type: "CreatePairingCode",
        user_id: user.id,
        device_name: null,
        grant: null,
        ttl_minutes: null,
      },
      "PairingCodeCreated",
    );
    await ctx.send({ type: "CancelPairingCode", pairing_id: second.pairing.id }, "Done");
    expect(findPairingCode(ctx.s, second.code)).toBeUndefined();

    await ctx.send(
      {
        type: "UpdateRemoteUser",
        user_id: user.id,
        update: { display_name: null, grant: null, disabled: true },
      },
      "RemoteUserSaved",
    );
    const disabled = await ctx.transport.send({
      type: "CreatePairingCode",
      user_id: user.id,
      device_name: null,
      grant: null,
      ttl_minutes: null,
    });
    expect(disabled).toMatchObject({ type: "Error", code: "invalid_request" });
  });

  it("decorates bodies with remote content and honours the most specific allow", async () => {
    const ctx = setup();
    const summary = ctx.transport.data.messages.find(
      (m) => ctx.transport.data.bodies.get(m.id)?.remote_content_blocked === true,
    );
    if (summary === undefined) throw new Error("no blocked message");
    const body = ctx.transport.data.bodies.get(summary.id);
    if (body === undefined) throw new Error("no body");
    const blocked = decorateRemoteContent(ctx.transport, body);
    expect(blocked.remote_content).toMatchObject({ resources: 3, allowed_by: null });
    expect(blocked.remote_content?.sender).toBe(summary.from?.email.toLowerCase());
    expect(blocked.remote_content_blocked).toBe(true);

    await ctx.send(
      { type: "AllowRemoteContent", message_id: summary.id, scope: "domain" },
      "RemoteContentAllowed",
    );
    expect(decorateRemoteContent(ctx.transport, body).remote_content?.allowed_by).toBe("domain");
    await ctx.send(
      { type: "AllowRemoteContent", message_id: summary.id, scope: "message" },
      "RemoteContentAllowed",
    );
    const allowed = decorateRemoteContent(ctx.transport, body);
    expect(allowed.remote_content?.allowed_by).toBe("message");
    expect(allowed.remote_content_blocked).toBe(false);

    const plain = { ...body, remote_content_blocked: false };
    expect(decorateRemoteContent(ctx.transport, plain)).toBe(plain);
  });

  it("narrows sessions and devices when a user's grant narrows", async () => {
    const ctx = setup();
    const user = await withUser(ctx, true);
    ctx.s.devices.push({
      id: "rdev-9",
      user_id: user.id,
      name: "Tablet",
      paired_at: "2026-09-29T10:00:00Z",
      last_seen_at: null,
      user_agent: null,
      last_ip: null,
      grant: presetGrant("full", { admin: true }),
      revoked_at: null,
    });
    await ctx.send(
      {
        type: "UpdateRemoteUser",
        user_id: user.id,
        update: { display_name: null, grant: presetGrant("read-only"), disabled: null },
      },
      "RemoteUserSaved",
    );
    const { devices } = await ctx.send(
      { type: "ListRemoteDevices", user_id: null },
      "RemoteDevices",
    );
    expect(devices[0]?.grant).toEqual(presetGrant("read-only"));
    const renamed = await ctx.send(
      { type: "UpdateRemoteDevice", device_id: "rdev-9", name: " Pad ", grant: null },
      "RemoteDeviceSaved",
    );
    expect(renamed.device.name).toBe("Pad");
  });
});
