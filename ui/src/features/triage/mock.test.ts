import { describe, expect, it } from "vitest";
import type { Event } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import {
  defaultJunkSettings,
  defaultNotificationSettings,
  junkIssues,
  notificationIssues,
  parseHhmm,
} from "./model";
import { TRIAGE_REQUESTS, isTriageRequest } from "./mock";

describe("triage model", () => {
  it("validates like the core", () => {
    expect(junkIssues(defaultJunkSettings())).toEqual([]);
    expect(junkIssues({ ...defaultJunkSettings(), threshold: 100, min_training: 0 })).toEqual([
      "threshold",
      "min-training",
    ]);
    expect(parseHhmm("22:00")).toBe(1320);
    expect(parseHhmm("24:00")).toBeNull();
    expect(parseHhmm("7:00")).toBeNull();
    const folder = { scope: { type: "folder" as const, folder_id: "f1" }, notify: false };
    expect(
      notificationIssues({
        ...defaultNotificationSettings(),
        quiet_hours: { start: "22:00", end: "22:00", days: [] },
        overrides: [folder, folder],
      }),
    ).toEqual(["empty-quiet-hours", "duplicate-override"]);
  });
});

describe("triage mock", () => {
  it("routes exactly its requests", () => {
    expect(new Set(TRIAGE_REQUESTS).size).toBe(TRIAGE_REQUESTS.length);
    expect(isTriageRequest({ type: "TestNotification" })).toBe(true);
    expect(isTriageRequest({ type: "Ping" })).toBe(false);
  });

  it("serves settings and pushes test notifications", async () => {
    const t = new MockTransport();
    const events: Event[] = [];
    t.onEvent((e) => events.push(e));
    const account = t.data.accountsConfig.accounts[0]?.id ?? "";
    expect(await t.send({ type: "GetJunkStatus", account_id: account })).toMatchObject({
      type: "JunkStatus",
      status: { account_id: account, ready: false },
    });
    expect(
      await t.send({
        type: "UpdateJunkSettings",
        account_id: account,
        settings: { ...defaultJunkSettings(), threshold: 10 },
      }),
    ).toMatchObject({ code: "invalid_request", localized: { id: "junk-issue-threshold" } });
    expect(
      await t.send({
        type: "MarkJunk",
        message_ids: [],
        junk: true,
      }),
    ).toMatchObject({ code: "unsupported" });
    const quiet = {
      ...defaultNotificationSettings(),
      quiet_hours: { start: "22:00", end: "07:00", days: [] },
    };
    await t.send({ type: "UpdateNotificationSettings", settings: quiet });
    expect(await t.send({ type: "GetNotificationSettings" })).toEqual({
      type: "NotificationSettings",
      settings: quiet,
    });
    expect(await t.send({ type: "TestNotification" })).toEqual({ type: "Done" });
    await Promise.resolve();
    expect(events.map((e) => e.type)).toEqual([
      "NotificationSettingsChanged",
      "NotificationRequested",
    ]);
    expect(await t.send({ type: "ListSnoozed", account_id: null })).toEqual({
      type: "Snoozed",
      messages: [],
    });
  });
});
