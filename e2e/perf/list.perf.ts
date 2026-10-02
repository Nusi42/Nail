/**
 * UI performance on the large data set (plan M4c WP123): time to the first
 * painted message-list rows and frame times while scrolling the
 * virtualized list deep into the Inbox, measured in the page
 * (`requestAnimationFrame` intervals) and recorded as a Chrome trace
 * (long renderer tasks). Metrics go to `report-ui.json` for
 * `nail-bench check`; the budgets are asserted here as well.
 */
import { writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Page } from "@playwright/test";
import {
  budgetMax,
  longRendererTasks,
  median,
  percentile,
  readRustReport,
  startServe,
  workDir,
  writeUiReport,
  type Metric,
  type PerfServer,
} from "./support";

const ROW = '[data-part="message-list.row"]';
/** Wheel steps while scrolling and pixels per step. */
const STEPS = 240;
const STEP_PX = 640;

let server: PerfServer | null = null;
const metrics: Metric[] = [];
const details: Record<string, unknown> = {};

type PerfWindow = Window & {
  __nailFirstRows?: number;
  __nailFrames?: number[];
  __nailRecording?: boolean;
};

test.beforeAll(async () => {
  const report = readRustReport();
  if (!report) throw new Error("no benchmark data: the global setup did not run nail-bench");
  details.messages = report.facts.messages;
  details.largestFolder = report.facts["largest-folder"];
  server = await startServe(report.facts["data-dir"], report.facts["config-dir"]);
});

test.afterAll(async () => {
  const file = writeUiReport(metrics, details);
  console.log(`UI report: ${file}`);
  await server?.stop();
});

function record(metric: string, value: number, samples: number): void {
  metrics.push({ metric, value, samples });
  const max = budgetMax(metric);
  console.log(`${metric}: ${value.toFixed(1)} (budget ${max})`);
  expect
    .soft(value, `${metric} ${value.toFixed(1)} is over its budget ${max}`)
    .toBeLessThanOrEqual(max);
}

/** Records when the first rows are in the DOM and painted (two frames later). */
async function watchFirstRows(page: Page): Promise<void> {
  await page.addInitScript((selector: string) => {
    const w = window as PerfWindow;
    const seen = () => {
      if (w.__nailFirstRows !== undefined || !document.querySelector(selector)) return false;
      requestAnimationFrame(() =>
        requestAnimationFrame(() => {
          w.__nailFirstRows ??= performance.now();
        }),
      );
      return true;
    };
    const observer = new MutationObserver(() => {
      if (seen()) observer.disconnect();
    });
    observer.observe(document, { childList: true, subtree: true });
  }, ROW);
}

test("first render of the large Inbox", async ({ browser }) => {
  const samples: number[] = [];
  for (let run = 0; run < 3; run++) {
    // A fresh context each time: no cached bundle, no warm queries.
    const context = await browser.newContext();
    const page = await context.newPage();
    await watchFirstRows(page);
    await page.goto(server!.url);
    await expect(page.locator(ROW).first()).toBeVisible({ timeout: 120_000 });
    await page.waitForFunction(() => (window as PerfWindow).__nailFirstRows !== undefined);
    samples.push(await page.evaluate(() => (window as PerfWindow).__nailFirstRows!));
    await context.close();
  }
  details.firstRenderSamples = samples;
  record("first-render-ms", median(samples), samples.length);
});

test("scrolling the large Inbox", async ({ page, browser }) => {
  await page.goto(server!.url);
  const list = page.locator('[data-part="message-list"]');
  await expect(page.locator(ROW).first()).toBeVisible({ timeout: 120_000 });
  await expect(list).toHaveAttribute("aria-busy", "false");
  await list.hover();
  // Warm up: the first page of rows and the scroll path.
  await page.mouse.wheel(0, STEP_PX);
  await page.waitForTimeout(300);

  await page.evaluate(() => {
    const w = window as PerfWindow;
    w.__nailFrames = [];
    w.__nailRecording = true;
    let last = performance.now();
    const tick = (now: number) => {
      w.__nailFrames!.push(now - last);
      last = now;
      if (w.__nailRecording) requestAnimationFrame(tick);
    };
    requestAnimationFrame(tick);
  });
  await browser.startTracing(page, {
    categories: ["devtools.timeline", "disabled-by-default-devtools.timeline.frame", "toplevel"],
  });
  const started = Date.now();
  for (let step = 0; step < STEPS; step++) {
    await page.mouse.wheel(0, STEP_PX);
    await page.waitForTimeout(16);
  }
  // Let the last page load and the list settle.
  await page.waitForTimeout(500);
  const elapsedMs = Date.now() - started;
  const frames = await page.evaluate(() => {
    const w = window as PerfWindow;
    w.__nailRecording = false;
    // The first interval spans the setup, not a frame.
    return (w.__nailFrames ?? []).slice(1);
  });
  const trace = await browser.stopTracing();
  const tracePath = path.join(workDir(), "scroll-trace.json");
  writeFileSync(tracePath, trace);

  const deepest = await page
    .locator(ROW)
    .evaluateAll((rows) => Math.max(...rows.map((r) => Number(r.getAttribute("data-index") ?? 0))));
  const scrollTop = await list.evaluate((el) => el.scrollTop);
  const longTasks = longRendererTasks(trace.toString("utf8"));
  Object.assign(details, {
    scrollFrames: frames.length,
    scrollElapsedMs: elapsedMs,
    scrollDeepestRow: deepest,
    scrollTopPx: scrollTop,
    traceLongTasksMs: longTasks,
    tracePath,
  });
  // The scroll really went deep (paging through the core on the way).
  expect(frames.length).toBeGreaterThan(60);
  expect(deepest).toBeGreaterThan(Math.min(1000, (details.messages as number) / 10));
  record("scroll-frame-p95-ms", percentile(frames, 95), frames.length);
  record("scroll-long-frames", frames.filter((f) => f > 50).length, frames.length);
});
