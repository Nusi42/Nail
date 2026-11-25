/**
 * The test host's task-reminder rule against the host's (R8 review): the
 * Rust test `task_reminder_vectors` (crates/nail-plugins/src/writes.rs)
 * writes what Nail accepts for each input; this port must answer alike.
 */
import { readFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { isUtcInstant, taskReminderIssue } from "./reminders";

const here = dirname(fileURLToPath(import.meta.url));
const vectors = JSON.parse(
  readFileSync(
    join(here, "../../../crates/nail-api/fixtures/ts-vectors/task-reminders.json"),
    "utf8",
  ),
) as { cases: { fn: string; args: [unknown, boolean]; out: boolean }[] };

describe("task reminders (vectors from the host's rule)", () => {
  it("has cases", () => {
    expect(vectors.cases.length).toBeGreaterThan(20);
  });

  it.each(vectors.cases.map((c) => [JSON.stringify(c.args), c] as const))(
    "%s",
    (_name, { fn, args, out }) => {
      expect(fn).toBe("taskRemindersAccepted");
      const [reminders, hasDue] = args;
      expect(taskReminderIssue(reminders, hasDue) === null).toBe(out);
    },
  );

  it("names why it refuses", () => {
    expect(taskReminderIssue([{ at: "2026-10-02T10:00:00+02:00" }], true)).toMatch(/UTC instant/);
    expect(isUtcInstant("2026-10-02T08:00:00Z")).toBe(true);
  });
});
