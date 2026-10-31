/**
 * The page's own diagnostics for the core's `app.log` (ADR 0207): its
 * start (a reload shows), its uncaught errors and unhandled rejections
 * (capped per minute, the rest counted) and, every
 * `UI_MEMORY_SAMPLE_MINUTES`, its memory use. A page that dies or hangs
 * after days hidden in the tray then leaves a trail; before, nothing in
 * Nail noticed.
 *
 * Reports go through `ReportUiDiagnostics`; a failing report is dropped
 * (never reported again, so no loop).
 */
import type { NailClient } from "../api/client";
import type { UiProblemKind, UiReport } from "../api/generated";
import {
  UI_MEMORY_SAMPLE_MINUTES,
  UI_PROBLEMS_PER_MINUTE,
  UI_REPORT_MAX_CHARS,
} from "../api/generated/constants";

const MINUTE_MS = 60_000;

/** What a report is sent with. */
export type ReportClient = Pick<NailClient, "call">;

/** An error's message and stack, cut to what the core logs. */
export function describeError(error: unknown): { message: string; stack?: string } {
  const cut = (text: string) => text.slice(0, UI_REPORT_MAX_CHARS);
  if (error instanceof Error) {
    const message = cut(`${error.name}: ${error.message}`);
    return error.stack ? { message, stack: cut(error.stack) } : { message };
  }
  if (typeof error === "string") return { message: cut(error) };
  try {
    // `JSON.stringify(undefined)` is undefined at run time, whatever its type says.
    const json = JSON.stringify(error) as string | undefined;
    return { message: cut(json ?? String(error)) };
  } catch {
    return { message: cut(String(error)) };
  }
}

/** Sends reports; problems are capped per minute. */
export class PageReporter {
  private windowStart = Number.NEGATIVE_INFINITY;
  private inWindow = 0;
  private suppressed = 0;

  constructor(
    private readonly client: ReportClient,
    private readonly now: () => number = () => Date.now(),
  ) {}

  /** Sends one report; failures are dropped. */
  send(report: UiReport): void {
    try {
      void this.client.call({ type: "ReportUiDiagnostics", report }).catch(() => undefined);
    } catch {
      // A closed client: nothing to report to.
    }
  }

  /** Reports an uncaught problem, unless this minute's cap is reached. */
  problem(kind: UiProblemKind, error: unknown): void {
    const now = this.now();
    if (now - this.windowStart >= MINUTE_MS) {
      this.windowStart = now;
      this.inWindow = 0;
    }
    if (this.inWindow >= UI_PROBLEMS_PER_MINUTE) {
      this.suppressed += 1;
      return;
    }
    this.inWindow += 1;
    const suppressed = this.suppressed;
    this.suppressed = 0;
    this.send({ type: "problem", kind, ...describeError(error), suppressed });
  }
}

/** The route's first segment (`#/mail`, `#/window`): no ids in the log. */
export function routeOf(hash: string): string {
  const segment = hash.replace(/^#\/?/, "").split(/[/?]/)[0] ?? "";
  return `#/${segment}`;
}

interface HeapInfo {
  usedJSHeapSize?: number;
  totalJSHeapSize?: number;
  jsHeapSizeLimit?: number;
}

const kib = (bytes: number | undefined): number | undefined =>
  typeof bytes === "number" && Number.isFinite(bytes) ? Math.round(bytes / 1024) : undefined;

/** The page's memory use now (`performance.memory` where the engine has it). */
export function memoryReport(doc: Document, uptimeMs: number, perf: unknown): UiReport {
  const memory = (perf as { memory?: HeapInfo } | undefined)?.memory;
  const report: UiReport = {
    type: "memory",
    uptime_minutes: Math.max(0, Math.floor(uptimeMs / MINUTE_MS)),
    hidden: doc.visibilityState === "hidden",
    dom_nodes: doc.getElementsByTagName("*").length,
  };
  const used = kib(memory?.usedJSHeapSize);
  const total = kib(memory?.totalJSHeapSize);
  const limit = kib(memory?.jsHeapSizeLimit);
  if (used !== undefined) report.heap_used_kib = used;
  if (total !== undefined) report.heap_total_kib = total;
  if (limit !== undefined) report.heap_limit_kib = limit;
  return report;
}

/** The reporter of the running page, for errors outside React's boundaries. */
let current: PageReporter | null = null;

/**
 * Reports a render error a boundary caught (one no boundary caught reaches
 * the `error` listener: React's default `onUncaughtError` reports it there).
 */
export function reportRenderError(error: unknown): void {
  current?.problem("render-error", error);
}

/**
 * Starts the diagnostics of this page: the start report, the `error` /
 * `unhandledrejection` listeners and the memory samples. Returns the stop.
 */
export function installPageDiagnostics(
  client: ReportClient,
  win: Window = window,
  now: () => number = () => Date.now(),
): () => void {
  const reporter = new PageReporter(client, now);
  const startedAt = now();
  current = reporter;
  reporter.send({ type: "started", route: routeOf(win.location.hash) });
  const onError = (event: ErrorEvent) => {
    reporter.problem("error", event.error ?? event.message);
  };
  const onRejection = (event: PromiseRejectionEvent) => {
    reporter.problem("unhandled-rejection", event.reason);
  };
  win.addEventListener("error", onError);
  win.addEventListener("unhandledrejection", onRejection);
  const timer = win.setInterval(() => {
    reporter.send(memoryReport(win.document, now() - startedAt, win.performance));
  }, UI_MEMORY_SAMPLE_MINUTES * MINUTE_MS);
  return () => {
    win.removeEventListener("error", onError);
    win.removeEventListener("unhandledrejection", onRejection);
    win.clearInterval(timer);
    if (current === reporter) current = null;
  };
}

/** How the page reloads (an object, so tests can watch it). */
export const pageControls = {
  reload(): void {
    window.location.reload();
  },
};

/** Reloads the page (the "Reload window" action and the error screen). */
export function reloadPage(): void {
  pageControls.reload();
}
