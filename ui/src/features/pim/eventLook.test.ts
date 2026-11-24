// @vitest-environment node
/**
 * The pure helpers of the quick look, the compact editor and the reminder
 * toast (R7, ADR 0246): lengths as text, the reminder chip, people
 * summaries and the call link — with the English catalog.
 */
import { FluentBundle, FluentResource } from "@fluent/bundle";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import type { Alarm } from "../../api/generated";
import type { MessageArgs } from "../../i18n/localizer";
import {
  alarmsForChoice,
  joinLinkOf,
  lengthParts,
  lengthText,
  minutesOf,
  myAttendee,
  peopleSummary,
  reminderChoiceOf,
  reminderChoices,
  reminderText,
} from "./eventLook";

const bundle = new FluentBundle("en", { useIsolating: false });
for (const file of ["calendar-look.ftl", "agents.ftl", "menus.ftl", "print.ftl", "common.ftl"]) {
  bundle.addResource(
    new FluentResource(readFileSync(resolve(process.cwd(), "../locales/en", file), "utf8")),
  );
}

/** English texts straight from the catalog. */
function t(id: string, args?: MessageArgs): string {
  const message = bundle.getMessage(id);
  if (!message?.value) throw new Error(`no message ${id}`);
  return bundle.formatPattern(message.value, args);
}

const relative = (minutes: number): Alarm => ({
  uid: "a1",
  action: "display",
  trigger: { type: "relative", offset_secs: -minutes * 60, related: "start" },
  description: null,
  repeat: null,
  acknowledged: null,
});

describe("lengths", () => {
  it("splits minutes into days, hours and minutes", () => {
    expect(lengthParts(90)).toEqual({ days: 0, hours: 1, minutes: 30 });
    expect(lengthParts(2 * 1440 + 15)).toEqual({ days: 2, hours: 0, minutes: 15 });
    expect(lengthParts(-5)).toEqual({ days: 0, hours: 0, minutes: 0 });
  });

  it("reads them as text in the UI language", () => {
    expect(lengthText(t, "en", 60)).toBe("1 hour");
    expect(lengthText(t, "en", 90)).toBe("1 hour 30 minutes");
    expect(lengthText(t, "en", 15)).toBe("15 minutes");
    expect(lengthText(t, "en", 2880)).toBe("2 days");
    expect(lengthText(t, "en", 0)).toBe("0 minutes");
    expect(minutesOf("2026-11-27T15:00:00Z", "2026-11-27T16:30:00Z")).toBe(90);
  });
});

describe("the reminder chip", () => {
  it("names one plain reminder before the start by its preset, anything else custom", () => {
    expect(reminderChoiceOf([])).toBe("none");
    expect(reminderChoiceOf([relative(15)])).toBe("15");
    expect(reminderChoiceOf([relative(0)])).toBe("0");
    expect(reminderChoiceOf([relative(1440)])).toBe("1440");
    expect(reminderChoiceOf([relative(7)])).toBe("custom");
    expect(reminderChoiceOf([relative(15), relative(60)])).toBe("custom");
    expect(reminderChoiceOf([{ ...relative(15), action: "email" }])).toBe("custom");
  });

  it("offers custom only while the alarms are custom", () => {
    expect(reminderChoices("15")).not.toContain("custom");
    expect(reminderChoices("custom").at(-1)).toBe("custom");
    expect(reminderChoices("none")[0]).toBe("none");
  });

  it("turns a choice into alarms, keeping the first alarm's uid", () => {
    expect(alarmsForChoice("none", [relative(15)])).toEqual([]);
    expect(alarmsForChoice("30", [relative(15)])).toEqual([relative(30)]);
    const custom = [relative(7), relative(60)];
    expect(alarmsForChoice("custom", custom)).toEqual(custom);
    expect(reminderChoiceOf(alarmsForChoice("60", []))).toBe("60");
  });

  it("reads the choices as text", () => {
    expect(reminderText(t, "en", "none")).toBe("No reminder");
    expect(reminderText(t, "en", "0")).toBe("At the start");
    expect(reminderText(t, "en", "30")).toBe("30 minutes before");
    expect(reminderText(t, "en", "1440")).toBe("1 day before");
    expect(reminderText(t, "en", "custom")).toBe("Custom reminders");
  });
});

describe("people and calls", () => {
  it("summarizes who answered what, leaving out zero counts", () => {
    expect(peopleSummary(t, "en", ["accepted", "needs-action", "delegated"])).toBe(
      "3 people · 1 accepted, 2 not answered",
    );
    expect(peopleSummary(t, "en", ["declined"])).toBe("1 person · 1 declined");
    expect(peopleSummary(t, "en", [])).toBe("0 people");
  });

  it("finds the user's own attendee entry by any of their addresses", () => {
    const attendee = {
      email: "Ada@Work.example",
      name: null,
      role: "required" as const,
      status: "needs-action" as const,
      kind: "individual" as const,
      rsvp: true,
      delegated_to: [],
      delegated_from: [],
    };
    expect(myAttendee([attendee], ["ada@work.example"])).toBe(attendee);
    expect(myAttendee([attendee], ["ada@example.org"])).toBeNull();
  });

  it("joins only https call links", () => {
    expect(joinLinkOf([{ uri: "tel:+123", label: null, features: ["audio"] }])).toBeNull();
    expect(
      joinLinkOf([
        { uri: "tel:+123", label: null, features: ["audio"] },
        { uri: " https://meet.example.com/x ", label: null, features: ["video"] },
      ]),
    ).toBe("https://meet.example.com/x");
  });
});
