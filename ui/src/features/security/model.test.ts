import type {
  AuthMethodResult,
  AuthResults,
  RecipientKeyStatus,
  RecipientKeysReport,
  SecurityStatus,
  SignatureInfo,
} from "../../api/generated";
import { makeKey } from "./mockSeed";
import {
  attachmentWarning,
  authSummary,
  authTone,
  bytesToBase64,
  decisionOf,
  distinctRecipients,
  exportHref,
  formatFingerprint,
  formatPasscode,
  isCompletePasscode,
  isDefaultCrypto,
  isPkcs12Name,
  isTrusted,
  keyState,
  looksArmored,
  ownKeysFor,
  resolveTechnology,
  securityVerdict,
  sendBlock,
  shortKeyId,
  sortKeys,
  defaultIdentityCrypto,
  warningsForLink,
} from "./model";

function sig(extra: Partial<SignatureInfo> = {}): SignatureInfo {
  return {
    state: "valid",
    signer: "grace@navy.example",
    signer_matches_from: true,
    key_id: "key-3",
    fingerprint: "AB".repeat(20),
    trust: "verified",
    signed_at: null,
    chain: null,
    ...extra,
  };
}

function status(extra: Partial<SecurityStatus> = {}): SecurityStatus {
  return {
    technology: "openpgp",
    signatures: [sig()],
    encryption: null,
    header_protection: "none",
    partial: false,
    inline_pgp: false,
    ...extra,
  };
}

const r = (verdict: AuthMethodResult["verdict"]): AuthMethodResult => ({
  verdict,
  domain: "x.example",
  selector: null,
  aligned: true,
  reason: null,
});

function auth(extra: Partial<AuthResults> = {}): AuthResults {
  return { authserv_id: "mx", dkim: [], spf: null, dmarc: null, arc: null, ...extra };
}

describe("security verdict", () => {
  it("shows a verified signature from a trusted key as good", () => {
    expect(securityVerdict(status())).toMatchObject({ signature: "verified", tone: "good" });
  });

  it("does not call an unverified key verified", () => {
    const v = securityVerdict(status({ signatures: [sig({ trust: "unknown" })] }));
    expect(v).toMatchObject({ signature: "valid", tone: "neutral" });
    expect(securityVerdict(status({ signatures: [sig({ trust: "autocrypt" })] })).signature).toBe(
      "verified",
    );
    expect(
      securityVerdict(status({ signatures: [sig({ trust: "autocrypt-gossip" })] })).signature,
    ).toBe("valid");
  });

  it("warns when someone else signed", () => {
    expect(
      securityVerdict(status({ signatures: [sig({ signer_matches_from: false })] })),
    ).toMatchObject({ signature: "mismatch", tone: "warning" });
  });

  it("treats a rejected signer and an invalid signature as danger", () => {
    expect(securityVerdict(status({ signatures: [sig({ trust: "rejected" })] })).tone).toBe(
      "danger",
    );
    expect(securityVerdict(status({ signatures: [sig({ state: "invalid" })] }))).toMatchObject({
      signature: "invalid",
      tone: "danger",
    });
  });

  it("lets the worst of several signatures win", () => {
    const v = securityVerdict(status({ signatures: [sig(), sig({ state: "invalid" })] }));
    expect(v.signature).toBe("invalid");
  });

  it("maps key problems and errors", () => {
    expect(securityVerdict(status({ signatures: [sig({ state: "key-revoked" })] })).signature).toBe(
      "key-problem",
    );
    expect(securityVerdict(status({ signatures: [sig({ state: "unsupported" })] })).signature).toBe(
      "error",
    );
    expect(securityVerdict(status({ signatures: [sig({ state: "unknown-key" })] }))).toMatchObject({
      signature: "unknown-key",
      tone: "neutral",
    });
  });

  it("uses the S/MIME chain instead of trust", () => {
    const chain = { state: "valid" as const, revocation: "not-checked" as const, certificates: [] };
    expect(
      securityVerdict(status({ signatures: [sig({ trust: "unknown", chain })] })).signature,
    ).toBe("verified");
    expect(
      securityVerdict(
        status({
          signatures: [sig({ trust: "unknown", chain: { ...chain, state: "untrusted-root" } })],
        }),
      ).signature,
    ).toBe("valid");
  });

  it("reports encryption states", () => {
    const enc = (state: "decrypted" | "locked" | "no-key" | "failed" | "unsupported") =>
      securityVerdict(
        status({
          signatures: [],
          encryption: { state, key_id: null, recipients: [], algorithm: null },
        }),
      );
    expect(enc("decrypted")).toMatchObject({ encryption: "decrypted", tone: "good" });
    expect(enc("locked")).toMatchObject({ encryption: "locked", tone: "warning" });
    expect(enc("no-key").tone).toBe("warning");
    expect(enc("failed").tone).toBe("danger");
    expect(enc("unsupported").encryption).toBe("failed");
  });

  it("never shows a partly protected or header-mismatched message as good", () => {
    expect(securityVerdict(status({ partial: true }))).toMatchObject({
      partial: true,
      tone: "warning",
    });
    expect(securityVerdict(status({ header_protection: "mismatch" }))).toMatchObject({
      headerMismatch: true,
      tone: "warning",
    });
  });
});

describe("auth summary", () => {
  it("follows the core: DMARC, else a passing DKIM, else the first DKIM, else SPF", () => {
    expect(authSummary(auth())).toBe("none");
    expect(authSummary(auth({ spf: r("softfail") }))).toBe("softfail");
    expect(authSummary(auth({ dkim: [r("fail"), r("pass")], spf: r("fail") }))).toBe("pass");
    expect(authSummary(auth({ dkim: [r("neutral")], spf: r("pass") }))).toBe("neutral");
    expect(authSummary(auth({ dkim: [r("pass")], dmarc: r("fail") }))).toBe("fail");
  });

  it("maps verdicts to tones", () => {
    expect(authTone("pass")).toBe("good");
    expect(authTone("fail")).toBe("danger");
    expect(authTone("permerror")).toBe("danger");
    expect(authTone("softfail")).toBe("warning");
    expect(authTone("none")).toBe("neutral");
  });
});

describe("warnings", () => {
  const warnings = [
    {
      kind: "link-mismatch" as const,
      link: 2,
      href: "https://a",
      shown_host: "b",
      target_host: "a",
    },
    { kind: "suspicious-link" as const, link: 2, href: "https://a" },
    {
      kind: "dangerous-attachment" as const,
      index: 1,
      filename: "x.exe",
      risk: "executable" as const,
    },
  ];
  it("finds warnings per link and per attachment", () => {
    expect(warningsForLink(warnings, 2)).toHaveLength(2);
    expect(warningsForLink(warnings, 0)).toHaveLength(0);
    expect(warningsForLink(undefined, 0)).toEqual([]);
    expect(attachmentWarning(warnings, 1)?.risk).toBe("executable");
    expect(attachmentWarning(warnings, 0)).toBeNull();
  });
});

describe("keys", () => {
  it("formats fingerprints and key ids", () => {
    expect(formatFingerprint("abcd ef01-2345")).toBe("ABCD EF01 2345");
    expect(shortKeyId("0123456789ABCDEF0123456789ABCDEF01234567")).toBe("89AB CDEF 0123 4567");
    expect(shortKeyId("0123456789ABCDEF0123456789ABCDEF01234567").replaceAll(" ", "")).toHaveLength(
      16,
    );
  });

  it("orders own keys first", () => {
    const a = makeKey("a", "openpgp", ["Zed <z@x.example>"], { has_secret: true });
    const b = makeKey("b", "openpgp", ["Amy <a@x.example>"]);
    expect(sortKeys([b, a]).map((k) => k.id)).toEqual(["a", "b"]);
  });

  it("derives the key state", () => {
    const k = makeKey("a", "openpgp", ["A <a@x.example>"]);
    expect(keyState(k)).toBe("ok");
    expect(keyState({ ...k, expired: true })).toBe("expired");
    expect(keyState({ ...k, revoked: true, expired: true })).toBe("revoked");
    expect(keyState({ ...k, trust: "rejected" })).toBe("rejected");
  });

  it("maps trust to decisions and trustworthiness", () => {
    expect(decisionOf("verified")).toBe("verified");
    expect(decisionOf("rejected")).toBe("rejected");
    expect(decisionOf("autocrypt")).toBe("unset");
    expect(isTrusted("autocrypt")).toBe(true);
    expect(isTrusted("autocrypt-gossip")).toBe(false);
    expect(isTrusted("own")).toBe(true);
  });

  it("offers only usable own keys of the identity's address", () => {
    const own = makeKey("a", "openpgp", ["Ada <ada@example.org>"], { has_secret: true });
    const expired = { ...own, id: "b", expired: true };
    const other = makeKey("c", "openpgp", ["Bob <bob@example.org>"], { has_secret: true });
    const pub = makeKey("d", "openpgp", ["Ada <ada@example.org>"]);
    expect(ownKeysFor([own, expired, other, pub], "openpgp", "ADA@example.org")).toEqual([own]);
    expect(ownKeysFor([own], "smime", "ada@example.org")).toEqual([]);
  });

  it("knows the identity defaults", () => {
    expect(isDefaultCrypto(defaultIdentityCrypto())).toBe(true);
    expect(isDefaultCrypto({ ...defaultIdentityCrypto(), sign: true })).toBe(false);
  });
});

function rk(email: string, openpgp: boolean, smime = false): RecipientKeyStatus {
  const key = (usable: boolean) => ({
    key_id: "k",
    fingerprint: "F",
    state: usable ? ("usable" as const) : ("expired" as const),
    trust: "unknown" as const,
    source: "imported" as const,
  });
  return {
    email,
    openpgp: openpgp ? key(true) : null,
    smime: smime ? key(true) : null,
    autocrypt: null,
    lookup: "not-attempted",
  };
}

function report(recipients: RecipientKeyStatus[], own = { openpgp: true, smime: false }) {
  const all = (tech: "openpgp" | "smime") =>
    recipients.length > 0 && recipients.every((x) => x[tech]?.state === "usable");
  const rep: RecipientKeysReport = {
    recipients,
    openpgp: { own_key: own.openpgp, all_recipients: all("openpgp") },
    smime: { own_key: own.smime, all_recipients: all("smime") },
    autocrypt: "disable",
    suggested: { sign: false, encrypt: false, technology: null, protect_headers: null },
  };
  return rep;
}

describe("send guard", () => {
  const encrypt = { sign: false, encrypt: true, technology: null, protect_headers: null };

  it("lets plain mail go", () => {
    expect(sendBlock({ ...encrypt, encrypt: false }, "auto", null)).toBeNull();
  });

  it("waits for the key check", () => {
    expect(sendBlock(encrypt, "auto", null)?.id).toBe("security-compose-block-checking");
  });

  it("names the recipients without a usable key", () => {
    const block = sendBlock(
      encrypt,
      "auto",
      report([rk("a@x.example", true), rk("b@x.example", false)]),
    );
    expect(block).toEqual({ id: "security-compose-block-missing", recipients: ["b@x.example"] });
  });

  it("needs an own key to sign or encrypt", () => {
    const rep = report([rk("a@x.example", true)], { openpgp: false, smime: false });
    expect(sendBlock({ ...encrypt, encrypt: false, sign: true }, "auto", rep)?.id).toBe(
      "security-compose-block-no-own-key-openpgp",
    );
  });

  it("needs a recipient to encrypt", () => {
    expect(sendBlock(encrypt, "auto", report([]))?.id).toBe("security-compose-block-no-recipients");
  });

  it("passes when every recipient has a key", () => {
    expect(sendBlock(encrypt, "auto", report([rk("a@x.example", true)]))).toBeNull();
  });

  it("resolves the technology: choice, preference, what works, OpenPGP", () => {
    const smimeOnly = report([rk("a@x.example", false, true)], { openpgp: true, smime: true });
    expect(resolveTechnology({ ...encrypt, technology: "smime" }, "openpgp", null)).toBe("smime");
    expect(resolveTechnology(encrypt, "smime", null)).toBe("smime");
    expect(resolveTechnology(encrypt, "auto", smimeOnly)).toBe("smime");
    expect(resolveTechnology(encrypt, "auto", report([], { openpgp: false, smime: true }))).toBe(
      "smime",
    );
    expect(resolveTechnology(encrypt, "auto", null)).toBe("openpgp");
  });

  it("dedupes recipients case-insensitively", () => {
    expect(distinctRecipients([" A@x.example", "a@x.example", "", "b@x.example"])).toEqual([
      "a@x.example",
      "b@x.example",
    ]);
  });
});

describe("helpers", () => {
  it("formats setup codes as nine blocks of four digits", () => {
    const digits = "123456789012345678901234567890123456";
    expect(formatPasscode(digits)).toBe("1234-5678-9012-3456-7890-1234-5678-9012-3456");
    expect(formatPasscode("12 34-5x")).toBe("1234-5");
    expect(formatPasscode(`${digits}999`)).toBe(formatPasscode(digits));
    expect(isCompletePasscode(digits)).toBe(true);
    expect(isCompletePasscode("1234")).toBe(false);
  });

  it("encodes files", () => {
    expect(bytesToBase64(new TextEncoder().encode("hello"))).toBe("aGVsbG8=");
    expect(bytesToBase64(new Uint8Array(70_000)).length).toBeGreaterThan(90_000);
    expect(isPkcs12Name("me.PFX")).toBe(true);
    expect(isPkcs12Name("me.asc")).toBe(false);
    expect(looksArmored("-----BEGIN PGP PUBLIC KEY BLOCK-----")).toBe(true);
    expect(looksArmored("binary")).toBe(false);
    expect(exportHref("application/pgp-keys", "text", "a b")).toBe(
      "data:application/pgp-keys;charset=utf-8,a%20b",
    );
    expect(exportHref("application/x", "base64", "QQ==")).toBe("data:application/x;base64,QQ==");
  });
});
