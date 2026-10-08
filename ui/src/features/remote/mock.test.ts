import type { RemoteGrant } from "../../api/generated";
import {
  REMOTE_REQUESTS,
  defaultRemoteUiSettings,
  intersectGrants,
  isRemoteRequest,
  stoppedStatus,
} from "./mock";
import { mockSetup } from "../../test/env";

function setup() {
  const core = mockSetup();
  return { ...core, s: core.transport.data.features.remote };
}

const readOnly: RemoteGrant = {
  mail: {
    scopes: ["read", "raw-source", "attachments"],
    all_accounts: true,
    accounts: [],
    excluded_folders: [],
    skip_draft_confirmation: false,
  },
  admin: false,
};

describe("remote mock", () => {
  it("routes exactly its requests", () => {
    expect(new Set(REMOTE_REQUESTS).size).toBe(REMOTE_REQUESTS.length);
    expect(isRemoteRequest({ type: "ListRemoteUsers" })).toBe(true);
    expect(isRemoteRequest({ type: "Ping" })).toBe(false);
  });

  it("serves the settings like the core: validated, internet mode locked", async () => {
    const { send, transport, events } = setup();
    const answer = await send({ type: "GetRemoteUiSettings" }, "RemoteUi");
    expect(answer.settings).toEqual(defaultRemoteUiSettings());
    expect(answer.status).toEqual(stoppedStatus());

    const lan = { ...defaultRemoteUiSettings(), enabled: true, bind_address: "0.0.0.0" };
    await send({ type: "UpdateRemoteUiSettings", settings: lan }, "RemoteUi");
    expect(events.map((e) => e.type)).toEqual(["RemoteUiStatusChanged"]);

    const plain = await transport.send({
      type: "UpdateRemoteUiSettings",
      settings: { ...lan, tls: "off" },
    });
    expect(plain).toMatchObject({ type: "Error", code: "invalid_request" });
    const internet = await transport.send({
      type: "UpdateRemoteUiSettings",
      settings: { ...lan, internet_mode: true },
    });
    expect(internet).toMatchObject({ type: "Error", code: "forbidden" });
    const again = await send({ type: "GetRemoteUiSettings" }, "RemoteUi");
    expect(again.settings).toEqual(lan);
  });

  it("manages users; the pre-WP45 switch answers unsupported", async () => {
    const { send, transport } = setup();
    const { user } = await send(
      { type: "CreateRemoteUser", name: "ada", display_name: null, grant: readOnly },
      "RemoteUserSaved",
    );
    expect(user.has_password).toBe(false);
    const dup = await transport.send({
      type: "CreateRemoteUser",
      name: "ADA",
      display_name: null,
      grant: readOnly,
    });
    expect(dup).toMatchObject({ type: "Error", code: "conflict" });
    const updated = await send(
      {
        type: "UpdateRemoteUser",
        user_id: user.id,
        update: { display_name: "Ada L.", grant: null, disabled: null },
      },
      "RemoteUserSaved",
    );
    expect(updated.user.display_name).toBe("Ada L.");
    transport.data.features.remote.unsupported = ["CreatePairingCode"];
    const pairing = await transport.send({
      type: "CreatePairingCode",
      user_id: user.id,
      device_name: null,
      grant: null,
      ttl_minutes: null,
    });
    expect(pairing).toMatchObject({ type: "Error", code: "unsupported" });
    await send({ type: "DeleteRemoteUser", user_id: user.id }, "Done");
    expect((await send({ type: "ListRemoteUsers" }, "RemoteUsers")).users).toEqual([]);
  });

  it("ends sessions of revoked devices with an event", async () => {
    const { send, s, events } = setup();
    s.devices.push({
      id: "rdev-1",
      user_id: "ruser-1",
      name: "Phone",
      paired_at: "2026-09-29T10:00:00Z",
      last_seen_at: null,
      user_agent: null,
      last_ip: null,
      grant: readOnly,
      revoked_at: null,
    });
    s.sessions.push({
      id: "rsess-1",
      user_id: "ruser-1",
      user_name: "ada",
      device_id: "rdev-1",
      device_name: "Phone",
      auth_method: "passkey",
      created_at: "2026-09-29T10:00:00Z",
      last_seen_at: "2026-09-29T10:00:00Z",
      expires_at: "2026-09-29T22:00:00Z",
      ip: null,
      user_agent: null,
      reauth_at: null,
      grant: readOnly,
      ended_at: null,
      end_reason: null,
    });
    await send({ type: "RevokeRemoteDevice", device_id: "rdev-1" }, "Done");
    expect(events).toContainEqual({
      type: "RemoteSessionEnded",
      session_id: "rsess-1",
      reason: "device-revoked",
    });
    const active = await send(
      {
        type: "ListRemoteSessions",
        filter: { user_id: null, device_id: null, include_ended: false },
      },
      "RemoteSessions",
    );
    expect(active.sessions).toEqual([]);
  });

  it("keeps remote-content allow-lists per message, sender and domain", async () => {
    const { send, transport, events } = setup();
    const message = transport.data.messages.find((m) => m.from !== null);
    if (message === undefined) throw new Error("no message with a sender");
    const sender = await send(
      { type: "AllowRemoteContent", message_id: message.id, scope: "sender" },
      "RemoteContentAllowed",
    );
    expect(sender.entry.value).toBe(message.from?.email.toLowerCase());
    await send(
      { type: "AllowRemoteContent", message_id: message.id, scope: "message" },
      "RemoteContentAllowed",
    );
    await send(
      { type: "AllowRemoteContent", message_id: message.id, scope: "domain" },
      "RemoteContentAllowed",
    );
    const { entries } = await send({ type: "ListRemoteContentAllows" }, "RemoteContentAllows");
    expect(entries.map((e) => e.scope)).toEqual(["message", "sender", "domain"]);
    expect(entries[0]?.label).toBe(message.subject);
    await send(
      { type: "RemoveRemoteContentAllow", scope: "sender", value: sender.entry.value },
      "Done",
    );
    expect(events.filter((e) => e.type === "RemoteContentPolicyChanged")).toHaveLength(4);
    const missing = await transport.send({
      type: "AllowRemoteContent",
      message_id: "nope",
      scope: "message",
    });
    expect(missing).toMatchObject({ type: "Error", code: "not_found" });
  });

  it("intersects grants like the core", () => {
    const full: RemoteGrant = {
      mail: { ...readOnly.mail, scopes: ["read", "send"], excluded_folders: ["f1"] },
      admin: true,
    };
    const both = intersectGrants(full, readOnly);
    expect(both.mail.scopes).toEqual(["read"]);
    expect(both.mail.excluded_folders).toEqual(["f1"]);
    expect(both.admin).toBe(false);
  });
});
