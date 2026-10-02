import { describe, expect, it } from "vitest";
import type { PrintOptions } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import { features } from "../registry";
import { runtimeIds } from "./messages";
import { isPrintRequest, printViewRoute, targetIssue } from "./mock";

const options: PrintOptions = {
  page_size: "a4",
  orientation: "portrait",
  headers: "basic",
  quoted_text: true,
  attachment_list: true,
  remote_images: false,
  backgrounds: false,
};

describe("print slot (M4c contract)", () => {
  it("is registered and lists its runtime ids", () => {
    expect(features.print).toBeDefined();
    expect(runtimeIds()).toContain("print-page-size-letter");
    expect(isPrintRequest({ type: "GetPrintJob", job_id: "x" })).toBe(true);
    expect(printViewRoute("p1")).toBe("#/print/p1");
  });

  it("checks agenda ranges like the core", () => {
    const agenda = (start: string, end: string) =>
      targetIssue({ type: "agenda", start, end, calendar_ids: [], layout: "week" });
    expect(agenda("2026-10-01", "2026-10-07")).toBeNull();
    expect(agenda("2026-10-07", "2026-10-01")).toBe("print-target-issue-end-before-start");
    expect(agenda("2025-01-01", "2026-12-31")).toBe("print-target-issue-range-too-long");
    expect(agenda("soon", "2026-12-31")).toBe("print-target-issue-invalid-date");
    expect(targetIssue({ type: "message", message_id: "m" })).toBeNull();
  });

  it("keeps print jobs and refuses PDFs without a desktop", async () => {
    const t = new MockTransport();
    const created = await t.send({
      type: "CreatePrintJob",
      target: { type: "message", message_id: "m1" },
      options,
    });
    if (created.type !== "PrintJob") throw new Error(`unexpected ${created.type}`);
    const jobId = created.job.job_id;
    expect(created.job.view).toBe(printViewRoute(jobId));
    expect(await t.send({ type: "PrintViewReady", job_id: jobId })).toEqual({ type: "Done" });
    expect(await t.send({ type: "GetPrintJob", job_id: jobId })).toMatchObject({
      job: { ready: true },
    });
    expect(await t.send({ type: "SavePrintPdf", job_id: jobId, path: "C:/x.pdf" })).toMatchObject({
      code: "unsupported",
      localized: { id: "desktop-unavailable" },
    });
    expect(await t.send({ type: "GetPrintJob", job_id: "nope" })).toMatchObject({
      code: "not_found",
      localized: { id: "print-job-expired" },
    });
  });
});
