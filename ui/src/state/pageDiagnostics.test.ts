/**
 * The page's diagnostics (ADR 0207): reports, the per-minute cap, the
 * memory sample, and a soak run of simulated days that must not pile up
 * timers or listeners.
 */
import { afterEach, describe, expect, it, vi } from "vitest";
import type { Request, Response, UiReport } from "../api/generated";
import {
  UI_MEMORY_SAMPLE_MINUTES,
  UI_PROBLEMS_PER_MINUTE,
  UI_REPORT_MAX_CHARS,
} from "../api/generated/constants";
import {
  PageReporter,
  describeError,
  installPageDiagnostics,
  memoryReport,
  reportRenderError,
  routeOf,
} from "./pageDiagnostics";

function recorder() {
  const reports: UiReport[] = [];
  const call = vi.fn((request: Request): Promise<Response> => {
    if (request.type === "ReportUiDiagnostics") reports.push(request.report);
    return Promise.resolve({ type: "Done" } as Response);
  });
  return { reports, client: { call } };
}

afterEach(() => {
  vi.useRealTimers();
});

describe("page diagnostics", () => {
  it("describes errors, strings and other values, cut to the core's limit", () => {
    const error = new TypeError("bad");
    expect(describeError(error)).toMatchObject({ message: "TypeError: bad" });
    expect(describeError(error).stack).toContain("bad");
    expect(describeError("plain")).toEqual({ message: "plain" });
    expect(describeError({ a: 1 })).toEqual({ message: '{"a":1}' });
    expect(describeError(undefined)).toEqual({ message: "undefined" });
    expect(describeError("x".repeat(UI_REPORT_MAX_CHARS + 10)).message).toHaveLength(
      UI_REPORT_MAX_CHARS,
    );
  });

  it("caps problems per minute and counts the rest", () => {
    const { reports, client } = recorder();
    let now = 0;
    const reporter = new PageReporter(client, () => now);
    for (let i = 0; i < UI_PROBLEMS_PER_MINUTE + 5; i++) reporter.problem("error", `e${String(i)}`);
    expect(reports).toHaveLength(UI_PROBLEMS_PER_MINUTE);
    now = 60_000;
    reporter.problem("unhandled-rejection", "later");
    expect(reports.at(-1)).toEqual({
      type: "problem",
      kind: "unhandled-rejection",
      message: "later",
      suppressed: 5,
    });
  });

  it("never throws when the client fails", async () => {
    const reporter = new PageReporter({ call: () => Promise.reject(new Error("closed")) });
    expect(() => {
      reporter.problem("error", "x");
    }).not.toThrow();
    const throwing = new PageReporter({
      call: () => {
        throw new Error("gone");
      },
    });
    expect(() => {
      throwing.send({ type: "started", route: "#/" });
    }).not.toThrow();
    await Promise.resolve();
  });

  it("names routes without ids", () => {
    expect(routeOf("")).toBe("#/");
    expect(routeOf("#/print/job-42")).toBe("#/print");
    expect(routeOf("#/window/w1?x=1")).toBe("#/window");
    expect(routeOf("#/mail")).toBe("#/mail");
  });

  it("samples the heap where the engine tells, the DOM always", () => {
    const report = memoryReport(document, 125 * 60_000, {
      memory: { usedJSHeapSize: 2048, totalJSHeapSize: 4096, jsHeapSizeLimit: 1024 * 1024 },
    });
    expect(report).toMatchObject({
      type: "memory",
      uptime_minutes: 125,
      heap_used_kib: 2,
      heap_total_kib: 4,
      heap_limit_kib: 1024,
    });
    const bare = memoryReport(document, 0, {});
    expect(bare).not.toHaveProperty("heap_used_kib");
    expect(bare.type === "memory" && bare.dom_nodes).toBeGreaterThan(0);
  });

  it("reports the start, uncaught errors, rejections and render errors", () => {
    const { reports, client } = recorder();
    const stop = installPageDiagnostics(client);
    expect(reports[0]).toEqual({ type: "started", route: routeOf(window.location.hash) });
    window.dispatchEvent(new ErrorEvent("error", { error: new Error("boom"), message: "boom" }));
    const rejection = new Event("unhandledrejection") as PromiseRejectionEvent;
    Object.defineProperty(rejection, "reason", { value: "nope" });
    window.dispatchEvent(rejection);
    reportRenderError(new Error("render"));
    expect(reports.slice(1).map((r) => r.type === "problem" && r.kind)).toEqual([
      "error",
      "unhandled-rejection",
      "render-error",
    ]);
    stop();
    window.dispatchEvent(new ErrorEvent("error", { message: "after" }));
    reportRenderError(new Error("after"));
    expect(reports).toHaveLength(4);
  });

  it("soak: three hidden days leave one timer, a bounded number of samples, and no listeners after stop", () => {
    vi.useFakeTimers();
    const { reports, client } = recorder();
    const add = vi.spyOn(window, "addEventListener");
    const remove = vi.spyOn(window, "removeEventListener");
    const stop = installPageDiagnostics(client);
    const days = 3;
    vi.advanceTimersByTime(days * 24 * 60 * 60_000);
    const samples = reports.filter((r) => r.type === "memory");
    expect(samples).toHaveLength((days * 24 * 60) / UI_MEMORY_SAMPLE_MINUTES);
    expect(vi.getTimerCount()).toBe(1);
    const last = samples.at(-1);
    expect(last?.type === "memory" && last.uptime_minutes).toBe(days * 24 * 60);
    stop();
    expect(vi.getTimerCount()).toBe(0);
    expect(remove.mock.calls.map((c) => c[0]).sort()).toEqual(
      add.mock.calls.map((c) => c[0]).sort(),
    );
    add.mockRestore();
    remove.mockRestore();
  });
});
