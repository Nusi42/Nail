import { WebAuthnError } from "@simplewebauthn/browser";
import type { MessageBody, RemoteUiSettings } from "../../api/generated";
import { defaultRemoteUiSettings } from "./mock";
import {
  extrasOf,
  formatPairingCode,
  grantIsWithin,
  groupFingerprint,
  isIpAddress,
  isLoopbackAddress,
  issueOfMessage,
  normalizePairingCode,
  presetGrant,
  presetOf,
  secondsUntil,
  settingsIssues,
} from "./model";
import { passkeyProblem } from "./passkeys";
import { qrPath } from "./QrCode";
import { allowValueFor, remoteContentView } from "./RemoteContent";
import { networksText, parseNetworks } from "./RemoteSettings";

const base = defaultRemoteUiSettings();
const with_ = (patch: Partial<RemoteUiSettings>): RemoteUiSettings => ({ ...base, ...patch });

describe("presets", () => {
  it("match RemoteAccessPreset::grant and never include the extras", () => {
    expect(presetGrant("read-only").mail.scopes).toEqual(["read", "raw-source", "attachments"]);
    expect(presetGrant("no-send").mail.scopes).not.toContain("send");
    expect(presetGrant("no-send").mail.scopes).toContain("organize");
    expect(presetGrant("full").mail.scopes).toEqual([
      "read",
      "raw-source",
      "attachments",
      "organize",
      "permanent-delete",
      "draft",
      "send",
      "calendar",
      "pim",
      "rules",
    ]);
    for (const p of ["read-only", "no-send", "full"] as const) {
      const g = presetGrant(p);
      expect(g.admin).toBe(false);
      expect(g.mail.all_accounts).toBe(true);
      expect(g.mail.scopes).not.toContain("encrypted");
      expect(g.mail.scopes).not.toContain("context");
    }
  });

  it("keeps explicit extras and recognizes presets", () => {
    const g = presetGrant("no-send", { admin: true, scopes: ["encrypted", "send"] });
    expect(g.admin).toBe(true);
    expect(g.mail.scopes).toContain("encrypted");
    expect(g.mail.scopes).not.toContain("send");
    expect(extrasOf(g)).toEqual(["encrypted"]);
    expect(presetOf(g)).toBe("no-send");
    expect(presetOf(presetGrant("full"))).toBe("full");
    const narrowed = presetGrant("full");
    narrowed.mail.all_accounts = false;
    narrowed.mail.accounts = ["acc-work"];
    expect(presetOf(narrowed)).toBe("custom");
    const odd = presetGrant("read-only");
    odd.mail.scopes = ["read"];
    expect(presetOf(odd)).toBe("custom");
  });

  it("checks that a grant stays within another", () => {
    expect(grantIsWithin(presetGrant("read-only"), presetGrant("full"))).toBe(true);
    expect(grantIsWithin(presetGrant("full"), presetGrant("no-send"))).toBe(false);
    expect(grantIsWithin(presetGrant("read-only", { admin: true }), presetGrant("full"))).toBe(
      false,
    );
    const outer = presetGrant("full");
    outer.mail.excluded_folders = ["f1"];
    expect(grantIsWithin(presetGrant("read-only"), outer)).toBe(false);
  });
});

describe("settingsIssues", () => {
  it("accepts the defaults", () => {
    expect(settingsIssues(base)).toEqual([]);
  });

  it("needs TLS off loopback and keeps internet mode locked", () => {
    expect(settingsIssues(with_({ bind_address: "0.0.0.0", tls: "off" }))).toEqual([
      "tls-required",
    ]);
    expect(settingsIssues(with_({ bind_address: "127.0.0.5", tls: "off" }))).toEqual([]);
    expect(settingsIssues(with_({ bind_address: "::1", tls: "off" }))).toEqual([]);
    expect(settingsIssues(with_({ internet_mode: true }))).toContain("internet-mode-locked");
    expect(settingsIssues(with_({ tls: "acme" }))).toEqual([
      "acme-needs-internet-mode",
      "acme-incomplete",
    ]);
  });

  it("checks addresses, ports, networks, host names and limits", () => {
    expect(settingsIssues(with_({ bind_address: "nail.local" }))).toEqual(["invalid-bind-address"]);
    expect(settingsIssues(with_({ bind_address: "256.1.1.1" }))).toEqual(["invalid-bind-address"]);
    expect(settingsIssues(with_({ port: 0 }))).toEqual(["invalid-port"]);
    expect(settingsIssues(with_({ port: 70000 }))).toEqual(["invalid-port"]);
    expect(settingsIssues(with_({ allowed_networks: ["192.168.1.0/24", "fd00::/8"] }))).toEqual([]);
    expect(settingsIssues(with_({ allowed_networks: ["192.168.1.0"] }))).toEqual([
      "invalid-network",
    ]);
    expect(settingsIssues(with_({ allowed_networks: ["10.0.0.0/33"] }))).toEqual([
      "invalid-network",
    ]);
    expect(settingsIssues(with_({ host_name: "nail-desktop.local" }))).toEqual([]);
    expect(settingsIssues(with_({ host_name: "192.168.1.2" }))).toEqual(["invalid-host-name"]);
    expect(settingsIssues(with_({ host_name: "-bad.local" }))).toEqual(["invalid-host-name"]);
    expect(settingsIssues(with_({ host_name: "host.123" }))).toEqual(["invalid-host-name"]);
    expect(settingsIssues(with_({ session_idle_minutes: 0 }))).toEqual(["invalid-session-limits"]);
    expect(settingsIssues(with_({ session_idle_minutes: 61, session_max_hours: 1 }))).toEqual([
      "invalid-session-limits",
    ]);
  });

  it("finds the issue a core error names", () => {
    expect(issueOfMessage("invalid-network")).toBe("invalid-network");
    expect(issueOfMessage("internet-mode-locked: internet mode is locked")).toBe(
      "internet-mode-locked",
    );
    expect(issueOfMessage("something else")).toBeNull();
  });

  it("recognizes IP and loopback addresses", () => {
    for (const ip of ["127.0.0.1", "0.0.0.0", "::", "::1", "fe80::1", "::ffff:192.168.1.1"]) {
      expect(isIpAddress(ip)).toBe(true);
    }
    for (const ip of ["1.2.3", "1:2:3:4:5:6:7:8:9", "a::b::c", "host"]) {
      expect(isIpAddress(ip)).toBe(false);
    }
    expect(isLoopbackAddress("127.1.2.3")).toBe(true);
    expect(isLoopbackAddress("::1")).toBe(true);
    expect(isLoopbackAddress("0.0.0.0")).toBe(false);
  });

  it("round-trips the allowed networks text", () => {
    expect(parseNetworks(" 10.0.0.0/8 \n\n192.168.0.0/16, fd00::/8")).toEqual([
      "10.0.0.0/8",
      "192.168.0.0/16",
      "fd00::/8",
    ]);
    expect(networksText(["a", "b"])).toBe("a\nb");
  });
});

describe("formatting", () => {
  it("groups fingerprints and pairing codes", () => {
    expect(groupFingerprint("ab12cd34ef")).toBe("AB12 CD34 EF");
    expect(groupFingerprint("AB:12:CD:34")).toBe("AB12 CD34");
    expect(groupFingerprint(" not hex ")).toBe("not hex");
    expect(normalizePairingCode(" k7qm-2wxr ")).toBe("K7QM2WXR");
    expect(normalizePairingCode("k7qm 2wxr")).toBe("K7QM2WXR");
    expect(formatPairingCode("k7qm2wxr")).toBe("K7QM-2WXR");
  });

  it("counts seconds until a time", () => {
    expect(secondsUntil("2026-01-01T00:00:10Z", Date.parse("2026-01-01T00:00:00Z"))).toBe(10);
    expect(secondsUntil("2026-01-01T00:00:00Z", Date.parse("2026-01-01T00:00:10Z"))).toBe(0);
    expect(secondsUntil("nonsense")).toBe(0);
  });

  it("draws a QR code with a quiet zone", () => {
    const { size, path } = qrPath("otpauth://totp/Nail:ada?secret=ABC");
    expect(size).toBeGreaterThanOrEqual(21 + 8);
    expect(path).toMatch(/^(M\d+ \d+h1v1h-1z)+$/);
    // The quiet zone stays empty: nothing at column 0.
    expect(path).not.toMatch(/M0 /);
  });
});

describe("passkey problems", () => {
  it("treats a closed prompt as cancelled, not as an error", () => {
    const aborted = new WebAuthnError({
      message: "x",
      code: "ERROR_CEREMONY_ABORTED",
      cause: new Error("x"),
    });
    expect(passkeyProblem(aborted)).toBe("cancelled");
    const denied = new WebAuthnError({
      message: "x",
      code: "ERROR_PASSTHROUGH_SEE_CAUSE_PROPERTY",
      cause: Object.assign(new Error("x"), { name: "NotAllowedError" }),
    });
    expect(passkeyProblem(denied)).toBe("cancelled");
    const domain = new WebAuthnError({
      message: "x",
      code: "ERROR_INVALID_DOMAIN",
      cause: new Error("x"),
    });
    expect(passkeyProblem(domain)).toBe("not-supported");
    expect(passkeyProblem(new DOMException("x", "NotAllowedError"))).toBe("cancelled");
    expect(passkeyProblem(new DOMException("x", "SecurityError"))).toBe("not-supported");
    expect(passkeyProblem("boom")).toBe("failed");
  });
});

describe("remoteContentView", () => {
  const body: MessageBody = {
    id: "m1",
    headers: {
      subject: "News",
      from: [{ name: "Shop", email: "News@Shop.Example" }],
      to: [],
      cc: [],
      reply_to: [],
      date: null,
      message_id: null,
    },
    text: "",
    sanitized_html: "<p>x</p>",
    attachments: [],
    remote_content_blocked: true,
  };

  it("falls back to the blocked flag and the From address", () => {
    const view = remoteContentView(body);
    expect(view).toEqual({
      blocked: true,
      resources: null,
      allowedBy: null,
      sender: "news@shop.example",
      domain: "shop.example",
    });
    expect(remoteContentView({ ...body, remote_content_blocked: false })).toBeNull();
    if (view === null) throw new Error("no view");
    expect(allowValueFor("m1", view, "message")).toBe("m1");
    expect(allowValueFor("m1", view, "domain")).toBe("shop.example");
  });

  it("prefers the core's remote_content", () => {
    const view = remoteContentView({
      ...body,
      remote_content_blocked: false,
      remote_content: {
        resources: 4,
        allowed_by: "sender",
        sender: "a@b.example",
        domain: "b.example",
      },
    });
    expect(view).toMatchObject({ blocked: false, resources: 4, allowedBy: "sender" });
    expect(
      remoteContentView({
        ...body,
        remote_content: { resources: 0, allowed_by: null, sender: null, domain: null },
      }),
    ).toBeNull();
  });
});
