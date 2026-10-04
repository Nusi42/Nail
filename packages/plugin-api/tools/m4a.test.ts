/**
 * The manifest checker's M4a rules (permissions, hooks, extensions, jobs,
 * MCP tools; same issue kinds and paths as `PluginManifest::validate`),
 * the cron parser (`CronSchedule::parse`) and the catalog loader.
 */
import { mkdirSync, mkdtempSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { describe, expect, it } from "vitest";

import type { CronError } from "./index.mjs";
import {
  checkManifest,
  cronMatches,
  FUTURE_PERMISSIONS,
  isCron,
  isExtension,
  isMimePattern,
  loadCatalogs,
  nextCronRun,
  parseCron,
  PERMISSIONS,
  WRITE_PERMISSIONS,
} from "./index.mjs";

const base = {
  id: "acme-crm",
  name: "Acme CRM",
  version: "1.0.0",
  apiVersion: "1",
  main: "dist/index.js",
};

const kinds = (manifest: unknown) =>
  checkManifest(manifest).map((i) => `${i.kind} ${i.path}${i.detail ? ` ${i.detail}` : ""}`);

describe("M4a permissions", () => {
  it("accepts every permission of the contract and refuses the future ones", () => {
    expect(PERMISSIONS).toHaveLength(18);
    expect(kinds({ ...base, permissions: [...PERMISSIONS, "network:api.acme.example"] })).toEqual(
      [],
    );
    expect(FUTURE_PERMISSIONS).toEqual([
      "mail.read.encrypted",
      "contacts.read",
      "tasks.read",
      "ui.webview",
    ]);
    expect(kinds({ ...base, permissions: FUTURE_PERMISSIONS })).toEqual([
      "permission-not-in-preview permissions[0] mail.read.encrypted",
      "permission-not-in-preview permissions[1] contacts.read",
      "permission-not-in-preview permissions[2] tasks.read",
      "permission-not-in-preview permissions[3] ui.webview",
    ]);
    expect(WRITE_PERMISSIONS).toEqual([
      "mail.organize",
      "mail.send",
      "calendar.write",
      "contacts.write",
      "tasks.write",
    ]);
  });

  it("reports permissions that need mail.read", () => {
    expect(
      kinds({
        ...base,
        permissions: ["mail.organize", "hooks.arrival", "hooks.open", "hooks.beforeSend"],
      }),
    ).toEqual([
      "missing-required-permission permissions mail.organize needs mail.read",
      "missing-required-permission permissions hooks.arrival needs mail.read",
      "missing-required-permission permissions hooks.open needs mail.read",
    ]);
    // mail.read.body keeps its own issue kind.
    expect(kinds({ ...base, permissions: ["mail.read.body"] })).toEqual([
      "body-without-read permissions mail.read.body",
    ]);
  });
});

describe("M4a contributions", () => {
  const all = {
    hooks: [
      { hook: "arrival", filter: "from:@acme.example" },
      { hook: "beforeSend", timeoutMs: 500 },
      { hook: "open" },
    ],
    composerActions: [{ id: "insert-ticket", title: "Insert ticket" }],
    markdownBlocks: [{ id: "ticket", title: "Ticket", template: "ACME-1" }],
    attachmentPreviews: [
      { id: "csv", title: "Table", mimeTypes: ["text/csv"], extensions: ["csv"], maxBytes: 1024 },
    ],
    attachmentActions: [{ id: "save", title: "Save to CRM", mimeTypes: ["application/*"] }],
    linkActions: [{ id: "open", title: "Open in CRM", hosts: ["*.acme.example"] }],
    jobs: [{ id: "sync", schedule: "*/15 * * * *", events: ["app.started"] }],
    mcpTools: [
      {
        id: "lookup",
        description: "Finds a customer",
        inputSchema: { type: "object" },
        scopes: ["read"],
      },
    ],
  };

  it("accepts valid contributions with their permissions", () => {
    const permissions = [
      "mail.read",
      "hooks.arrival",
      "hooks.beforeSend",
      "hooks.open",
      "composer",
      "attachments",
      "background",
      "mcp.expose",
    ];
    expect(kinds({ ...base, permissions, contributes: all })).toEqual([]);
  });

  it("needs a permission for each kind of contribution", () => {
    expect(kinds({ ...base, permissions: ["mail.read"], contributes: all })).toEqual([
      "contribution-needs-permission contributes.hooks hooks.arrival",
      "contribution-needs-permission contributes.hooks hooks.beforeSend",
      "contribution-needs-permission contributes.hooks hooks.open",
      "contribution-needs-permission contributes.composerActions composer",
      "contribution-needs-permission contributes.markdownBlocks composer",
      "contribution-needs-permission contributes.attachmentPreviews attachments",
      "contribution-needs-permission contributes.attachmentActions attachments",
      "contribution-needs-permission contributes.jobs background",
      "contribution-needs-permission contributes.mcpTools mcp.expose",
    ]);
  });

  it("reports invalid and duplicate hooks", () => {
    const permissions = ["mail.read", "hooks.arrival", "hooks.beforeSend", "hooks.open"];
    const hooks = [
      { hook: "beforeSend", filter: "has:attachment" },
      { hook: "open", timeoutMs: 10 },
      { hook: "open", timeoutMs: 60_000 },
      { hook: "arrival", filter: " " },
      { hook: "sometimes" },
      { hook: "arrival" },
      { hook: "arrival" },
    ];
    expect(kinds({ ...base, permissions, contributes: { hooks } })).toEqual([
      "invalid-hook contributes.hooks[0] beforeSend",
      "invalid-hook contributes.hooks[1] open",
      "invalid-hook contributes.hooks[2] open",
      "invalid-hook contributes.hooks[3] arrival",
      "invalid-hook contributes.hooks[4] sometimes",
      "duplicate-hook contributes.hooks[6] arrival",
    ]);
  });

  it("reports invalid and duplicate extensions", () => {
    const permissions = ["composer", "attachments"];
    const contributes = {
      composerActions: [
        { id: "a", title: "A" },
        { id: "a", title: "A again" },
        { id: "Bad", title: "B" },
        { id: "c", title: " " },
      ],
      markdownBlocks: [{ id: "b", title: "B", template: "x".repeat(1001) }],
      attachmentPreviews: [
        { id: "none", title: "No matcher" },
        { id: "big", title: "Too big", extensions: ["csv"], maxBytes: 11 * 1024 * 1024 },
        { id: "svg", title: "Bad type", mimeTypes: ["Image/SVG"] },
        { id: "dot", title: "Bad ext", extensions: [".csv"] },
      ],
      attachmentActions: [
        { id: "x", title: "X", mimeTypes: Array.from({ length: 21 }, () => "text/plain") },
      ],
      linkActions: [{ id: "l", title: "L", hosts: ["https://jira.example"] }],
    };
    expect(kinds({ ...base, permissions, contributes })).toEqual([
      "duplicate-extension contributes.composerActions[1] a",
      "invalid-extension contributes.composerActions[2] Bad",
      "invalid-extension contributes.composerActions[3] c",
      "invalid-extension contributes.markdownBlocks[0] b",
      "invalid-extension contributes.attachmentPreviews[0] none",
      "invalid-extension contributes.attachmentPreviews[1] big",
      "invalid-extension contributes.attachmentPreviews[2] svg",
      "invalid-extension contributes.attachmentPreviews[3] dot",
      "invalid-extension contributes.attachmentActions[0] x",
      "invalid-extension contributes.linkActions[0] l",
    ]);
  });

  it("reports invalid jobs and MCP tools", () => {
    const permissions = ["background", "mcp.expose"];
    const contributes = {
      jobs: [
        { id: "nothing" },
        { id: "often", schedule: "* * * * *" },
        { id: "bad-event", events: ["mail.deleted"] },
        { id: "ok", schedule: "@daily" },
        { id: "ok", events: ["message.received"] },
      ],
      mcpTools: [
        { id: "no-description", description: " " },
        { id: "array-schema", description: "x", inputSchema: { type: "array" } },
        { id: "no-scopes", description: "x", scopes: [] },
        { id: "bad-scope", description: "x", scopes: ["root"] },
        { id: `long-${"x".repeat(60)}`, description: "x" },
        { id: "fine", description: "x" },
        { id: "fine", description: "y" },
      ],
    };
    expect(kinds({ ...base, permissions, contributes })).toEqual([
      "invalid-job contributes.jobs[0] nothing",
      "invalid-job contributes.jobs[1] often",
      "invalid-job contributes.jobs[2] bad-event",
      "duplicate-job contributes.jobs[4] ok",
      "invalid-mcp-tool contributes.mcpTools[0] no-description",
      "invalid-mcp-tool contributes.mcpTools[1] array-schema",
      "invalid-mcp-tool contributes.mcpTools[2] no-scopes",
      "invalid-mcp-tool contributes.mcpTools[3] bad-scope",
      `invalid-mcp-tool contributes.mcpTools[4] long-${"x".repeat(60)}`,
      "duplicate-mcp-tool contributes.mcpTools[6] fine",
    ]);
  });

  it("counts contributions", () => {
    const jobs = Array.from({ length: 11 }, (_, i) => ({
      id: `j${String(i)}`,
      schedule: "@hourly",
    }));
    expect(kinds({ ...base, permissions: ["background"], contributes: { jobs } })).toContain(
      "too-many-contributions contributes.jobs",
    );
    // 51 distinct language tags: "laa", "lba", …
    const tags: Record<string, string> = {};
    for (let i = 0; i < 51; i += 1) {
      const tag = `l${String.fromCharCode(97 + (i % 26))}${String.fromCharCode(97 + Math.floor(i / 26))}`;
      tags[tag] = "locales/x.ftl";
    }
    expect(kinds({ ...base, contributes: { locales: tags } })).toEqual([
      "too-many-contributions contributes.locales",
    ]);
  });

  it("matches MIME types and extensions like Nail", () => {
    for (const ok of ["text/csv", "image/*", "application/vnd.ms-excel", "application/ld+json"]) {
      expect(isMimePattern(ok), ok).toBe(true);
    }
    for (const bad of ["text", "*/*", "Text/CSV", "text/csv/x", "", "text/"]) {
      expect(isMimePattern(bad), bad).toBe(false);
    }
    expect(isExtension("csv")).toBe(true);
    expect(isExtension("tar2")).toBe(true);
    for (const bad of [".csv", "CSV", "", "x".repeat(17), "tar.gz"]) {
      expect(isExtension(bad), bad).toBe(false);
    }
  });
});

describe("cron schedules", () => {
  it("parses fields, ranges, steps, lists and names", () => {
    const quarter = parseCron("*/15 * * * *");
    expect([...quarter.minutes]).toEqual([0, 15, 30, 45]);
    const workdays = parseCron("0 8 * * 1-5");
    expect([...workdays.weekdays]).toEqual([1, 2, 3, 4, 5]);
    expect([...parseCron("30 6 * * 7").weekdays]).toEqual([0]);
    expect([...parseCron("5/20 * * * *").minutes]).toEqual([5, 25, 45]);
    expect([...parseCron("0 9-17/4 * * *").hours]).toEqual([9, 13, 17]);
    expect([...parseCron("0,30 * * * *").minutes]).toEqual([0, 30]);
    expect(parseCron("@hourly").minutes).toEqual(new Set([0]));
    expect([...parseCron("@weekly").weekdays]).toEqual([0]);
    expect(isCron("@daily")).toBe(true);
  });

  it("refuses what Nail refuses", () => {
    const issue = (text: string) => {
      try {
        parseCron(text);
        return "ok";
      } catch (error) {
        return (error as CronError).issue;
      }
    };
    expect(issue("* * * *")).toBe("field-count");
    expect(issue("@yearly")).toBe("field-count");
    expect(issue("60 * * * *")).toBe("invalid-field");
    expect(issue("0 24 * * *")).toBe("invalid-field");
    expect(issue("0 0 0 * *")).toBe("invalid-field");
    expect(issue("0 0 * 13 *")).toBe("invalid-field");
    expect(issue("0 0 * * 8")).toBe("invalid-field");
    expect(issue("*/0 * * * *")).toBe("invalid-field");
    expect(issue("10-5 * * * *")).toBe("invalid-field");
    expect(issue("1-2-3 * * * *")).toBe("invalid-field");
    expect(issue("a * * * *")).toBe("invalid-field");
    expect(issue("* * * * *")).toBe("too-frequent");
    expect(issue("*/4 * * * *")).toBe("too-frequent");
    expect(issue("0,3 * * * *")).toBe("too-frequent");
    // The wrap into the next hour / day counts too.
    expect(issue("58,2 * * * *")).toBe("too-frequent");
    expect(issue("*/5 * * * *")).toBe("ok");
    expect(isCron(42)).toBe(false);
  });

  it("matches local times and finds the next run", () => {
    const workdays = parseCron("0 8 * * 1-5");
    expect(cronMatches(workdays, new Date(2026, 9, 2, 8, 0))).toBe(true); // a Friday
    expect(cronMatches(workdays, new Date(2026, 9, 3, 8, 0))).toBe(false); // Saturday
    expect(nextCronRun(workdays, new Date(2026, 9, 2, 8, 0))).toEqual(new Date(2026, 9, 5, 8, 0));
    // Either day field matches when both are restricted.
    const either = parseCron("0 12 1 * 1");
    expect(cronMatches(either, new Date(2026, 9, 1, 12, 0))).toBe(true); // the 1st (a Thursday)
    expect(cronMatches(either, new Date(2026, 9, 5, 12, 0))).toBe(true); // a Monday
    expect(cronMatches(either, new Date(2026, 9, 6, 12, 0))).toBe(false);
    expect(nextCronRun(parseCron("0 0 29 2 *"), new Date(2026, 0, 1))).toEqual(
      new Date(2028, 1, 29),
    );
    expect(nextCronRun(parseCron("0 0 31 4 *"), new Date(2026, 0, 1))).toBeNull(); // 31 April never comes
  });
});

describe("loadCatalogs", () => {
  it("reads the manifest's catalogs", () => {
    const dir = mkdtempSync(path.join(tmpdir(), "nail-catalogs-"));
    try {
      mkdirSync(path.join(dir, "locales"));
      writeFileSync(path.join(dir, "locales", "en.ftl"), "hello = Hello");
      writeFileSync(path.join(dir, "locales", "de.ftl"), "hello = Hallo");
      const manifest = {
        ...base,
        contributes: { locales: { en: "locales/en.ftl", de: "locales/de.ftl" } },
      };
      writeFileSync(path.join(dir, "manifest.json"), JSON.stringify(manifest));
      expect(loadCatalogs(dir)).toEqual({ en: "hello = Hello", de: "hello = Hallo" });
      expect(loadCatalogs(dir, { contributes: {} })).toEqual({});
      expect(() => loadCatalogs(dir, { contributes: { locales: { en: "../x.ftl" } } })).toThrow(
        /invalid catalog/,
      );
    } finally {
      rmSync(dir, { recursive: true, force: true });
    }
  });
});
