import { describe, expect, it } from "vitest";
import type { AccountConfig, Event, Response } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import { defaultVacation, isIsoDate, vacationIssues } from "./model";

function transport(): MockTransport {
  const t = new MockTransport();
  const work = t.data.accountsConfig.accounts.find((a) => a.incoming.protocol === "imap");
  if (!work) throw new Error("the mock has no IMAP account");
  work.sieve = { enabled: null, host: null, port: null, tls: null, script_name: null };
  return t;
}

function imapAccount(t: MockTransport): AccountConfig {
  const account = t.data.accountsConfig.accounts.find((a) => a.sieve !== undefined);
  if (!account) throw new Error("no sieve account");
  return account;
}

describe("sieve model", () => {
  it("validates vacation settings like the core", () => {
    expect(vacationIssues(defaultVacation())).toEqual([]);
    expect(vacationIssues({ ...defaultVacation(), enabled: true })).toEqual(["empty-body"]);
    expect(
      vacationIssues({
        ...defaultVacation(),
        days: 0,
        start_date: "2026-10-10",
        end_date: "2026-10-01",
        addresses: ["nope"],
        subject: "a\nb",
      }),
    ).toEqual(["days", "end-before-start", "address", "subject"]);
    expect(vacationIssues({ ...defaultVacation(), start_date: "2026-13-01" })).toEqual(["date"]);
    expect(isIsoDate("2026-02-28")).toBe(true);
    expect(isIsoDate("2026-2-28")).toBe(false);
  });
});

describe("sieve mock", () => {
  it("reports the state of accounts with ManageSieve settings", async () => {
    const t = transport();
    const account = imapAccount(t);
    const response = await t.send({ type: "GetSieveStatus", account_id: null });
    expect(response).toMatchObject({
      type: "SieveStatus",
      accounts: [{ account_id: account.id, state: "pending", script_name: "nail" }],
    });
    const synced = await t.send({ type: "SyncSieve", account_id: account.id });
    expect(synced).toMatchObject({
      type: "SieveStatus",
      accounts: [{ account_id: account.id, state: "in-sync", other_scripts: ["roundcube"] }],
    });
    const preview = await t.send({ type: "PreviewSieveScript", account_id: account.id });
    expect(preview).toMatchObject({ type: "SieveScript", account_id: account.id });
    t.data.features.sieve.failWith = "network";
    expect(await t.send({ type: "GetSieveStatus", account_id: account.id })).toMatchObject({
      accounts: [{ state: "failed", error: "network" }],
    });
    const pop = t.data.accountsConfig.accounts.find((a) => a.incoming.protocol !== "imap");
    if (pop) {
      expect(await t.send({ type: "SyncSieve", account_id: pop.id })).toMatchObject({
        code: "unsupported",
      });
    }
  });

  it("validates and stores vacation replies", async () => {
    const t = transport();
    const account = imapAccount(t);
    const events: Event[] = [];
    t.onEvent((e) => events.push(e));
    const bad: Response = await t.send({
      type: "UpdateVacation",
      account_id: account.id,
      settings: { ...defaultVacation(), enabled: true },
    });
    expect(bad).toMatchObject({
      type: "Error",
      code: "invalid_request",
      localized: { id: "vacation-issue-empty-body" },
    });
    const settings = { ...defaultVacation(), enabled: true, body: "Away" };
    await t.send({ type: "UpdateVacation", account_id: account.id, settings });
    expect(events).toContainEqual({ type: "SieveStatusChanged", account_id: account.id });
    expect(await t.send({ type: "GetVacation", account_id: account.id })).toEqual({
      type: "Vacation",
      account_id: account.id,
      settings,
    });
    expect(await t.send({ type: "GetVacation", account_id: "nobody" })).toMatchObject({
      code: "not_found",
    });
  });
});
