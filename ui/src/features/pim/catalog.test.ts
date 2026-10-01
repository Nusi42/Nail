// @vitest-environment node
/**
 * The feature's English catalog parses without errors, the German one is
 * empty (English fallback per key), and the messages with selectors say
 * the right thing for their arguments.
 */
import { FluentBundle, FluentResource } from "@fluent/bundle";
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import type { MessageArgs } from "../../i18n/localizer";

const localesDir = resolve(process.cwd(), "../locales");

function bundle(): FluentBundle {
  const text = readFileSync(resolve(localesDir, "en/pim.ftl"), "utf8");
  const resource = new FluentResource(text);
  const b = new FluentBundle("en", { useIsolating: false });
  const errors = b.addResource(resource);
  expect(errors).toEqual([]);
  return b;
}

function format(b: FluentBundle, id: string, args: MessageArgs = {}): string {
  const message = b.getMessage(id);
  expect(message?.value, id).toBeTruthy();
  const errors: Error[] = [];
  const text = b.formatPattern(message?.value ?? "", args, errors);
  expect(errors, id).toEqual([]);
  return text;
}

describe("pim catalog", () => {
  it("parses every entry (no junk)", () => {
    const text = readFileSync(resolve(localesDir, "en/pim.ftl"), "utf8");
    const resource = new FluentResource(text);
    const ids = resource.body.map((e) => e.id);
    expect(new Set(ids).size).toBe(ids.length);
    // Every non-comment line starting an entry made it into the resource.
    const declared = text.split("\n").filter((l) => /^[a-z][a-z0-9_-]* =/.test(l)).length;
    expect(ids.length).toBe(declared);
    bundle();
  });

  it("keeps the German file empty", () => {
    const text = readFileSync(resolve(localesDir, "de/pim.ftl"), "utf8");
    expect(new FluentResource(text).body).toEqual([]);
  });

  it("describes recurrence rules", () => {
    const b = bundle();
    const base = {
      mode: "day",
      monthday: 5,
      ordinal: 1,
      weekday: "Monday",
      days: "Monday",
      count: 0,
      until: "",
    };
    expect(
      format(b, "pim-recurrence-summary", { ...base, freq: "daily", interval: 1, end: "never" }),
    ).toBe("Every day.");
    expect(
      format(b, "pim-recurrence-summary", {
        ...base,
        freq: "weekly",
        interval: 2,
        days: "Monday, Thursday",
        end: "count",
        count: 4,
      }),
    ).toBe("Every 2 weeks on Monday, Thursday, 4 times.");
    expect(
      format(b, "pim-recurrence-summary", {
        ...base,
        freq: "monthly",
        interval: 1,
        mode: "weekday",
        ordinal: -1,
        weekday: "Friday",
        end: "until",
        until: "Dec 31, 2026",
      }),
    ).toBe("Every month on the last Friday, until Dec 31, 2026.");
    expect(
      format(b, "pim-recurrence-summary", { ...base, freq: "monthly", interval: 3, end: "never" }),
    ).toBe("Every 3 months on day 5.");
    expect(
      format(b, "pim-recurrence-summary", {
        ...base,
        freq: "yearly",
        interval: 1,
        end: "count",
        count: 1,
      }),
    ).toBe("Every year, once.");
  });

  it("names events, tasks and merges for screen readers", () => {
    const b = bundle();
    expect(
      format(b, "pim-event-label", {
        title: "Lunch",
        time: "12:30 – 13:30",
        day: "Oct 1",
        calendar: "Personal",
        recurring: "yes",
      }),
    ).toBe("Lunch, 12:30 – 13:30, Oct 1, Personal, repeats");
    expect(
      format(b, "pim-event-label", {
        title: "Lunch",
        time: "All day",
        day: "Oct 1",
        calendar: "none",
        recurring: "no",
      }),
    ).toBe("Lunch, All day, Oct 1");
    expect(
      format(b, "pim-task-label", {
        title: "Report",
        status: "in-process",
        due: "Oct 1",
        priority: "high",
      }),
    ).toBe("Report, in progress, due Oct 1, high priority");
    expect(
      format(b, "pim-task-label", {
        title: "Report",
        status: "needs-action",
        due: "none",
        priority: "none",
      }),
    ).toBe("Report");
    expect(format(b, "pim-merge-effect", { folded: 1, linked: 2 })).toBe(
      "One contact in the same address book is merged into it. 2 contacts in other address books stay there, linked to it.",
    );
    expect(
      format(b, "pim-import-summary", {
        name: "none",
        events: 1,
        tasks: 0,
        contacts: 0,
        groups: 0,
      }),
    ).toBe("events: 1, tasks: 0, contacts: 0, groups: 0.");
  });
});
