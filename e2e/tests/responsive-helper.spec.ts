import { existsSync } from "node:fs";
import { expect, test, type Page } from "@playwright/test";
import {
  STANDARD_SIZES,
  checkResponsive,
  formatIssues,
  layoutIssues,
  resizeWindow,
  screenshotPath,
  type ResponsiveIssue,
} from "../support/responsive";

/**
 * Self-test of the P1 responsive helper (ADR 0139) on static pages: every
 * check finds what it is meant to find and nothing on a clean layout.
 */

const BASE = `<style>
  html, body { margin: 0; height: 100%; overflow: hidden; font: 14px sans-serif; }
  [data-part="app"] { display: flex; flex-direction: column; gap: 8px; padding: 8px; }
</style>`;

async function show(page: Page, body: string): Promise<void> {
  await page.setViewportSize({ width: 800, height: 600 });
  await page.setContent(`<!doctype html><html><head>${BASE}</head><body>${body}</body></html>`);
}

const kinds = (issues: readonly ResponsiveIssue[]) => issues.map((i) => i.kind).sort();

test("a clean layout has no issues", async ({ page }) => {
  await show(
    page,
    `<div data-part="app">
      <div data-part="toolbar"><button data-part="toolbar.button">Reply</button> <button data-part="toolbar.button">Forward</button></div>
      <p data-part="text">A paragraph that wraps normally inside the page width.</p>
      <span data-part="snippet" style="display:block; width:120px; white-space:nowrap; overflow:hidden; text-overflow:ellipsis">A long snippet cut with an ellipsis on purpose</span>
      <div data-part="list" style="height:60px; overflow:auto"><div data-part="row" style="height:200px">Rows scroll inside their pane</div></div>
    </div>`,
  );
  const issues = await layoutIssues(page);
  expect(issues, formatIssues(issues)).toEqual([]);
});

test("horizontal page overflow and a scrolling page are found", async ({ page }) => {
  await show(
    page,
    `<style>html, body { overflow: auto; }</style>
     <div data-part="app"><div data-part="wide" style="width:1200px; height:900px">wide</div></div>`,
  );
  const issues = await layoutIssues(page, { skip: ["offscreen"] });
  expect(kinds(issues)).toEqual(["page-overflow", "page-scroll"]);
});

test("clipped text is found, an ellipsis and a line clamp are not", async ({ page }) => {
  await show(
    page,
    `<div data-part="app">
      <label data-part="form.label" style="display:block; width:60px; white-space:nowrap; overflow:hidden">Server address</label>
      <label data-part="form.label-ok" style="display:block; width:60px; white-space:nowrap; overflow:hidden; text-overflow:ellipsis">Server address</label>
      <p data-part="form.cut" style="width:100px; height:18px; overflow:hidden">A description that needs three lines in this narrow box</p>
      <p data-part="form.clamped" style="width:100px; overflow:hidden; display:-webkit-box; -webkit-box-orient:vertical; -webkit-line-clamp:1">A description that needs three lines in this narrow box</p>
    </div>`,
  );
  const issues = await layoutIssues(page);
  expect(kinds(issues)).toEqual(["clipped", "clipped"]);
  expect(issues.map((i) => i.element).join("\n")).toContain("form.label");
  expect(issues.map((i) => i.element).join("\n")).toContain("form.cut");
});

test("text sticking out of a box that does not clip it is found", async ({ page }) => {
  await show(
    page,
    `<div data-part="app">
      <label data-part="form.wide" style="display:block; width:40px; white-space:nowrap">Server address</label>
      <label data-part="form.fits" style="display:block; width:200px; white-space:nowrap">Server address</label>
      <label data-part="form.wraps" style="display:block; width:60px">Server address</label>
      <span data-part="form.ellipsis" style="display:block; width:40px; white-space:nowrap; overflow:hidden; text-overflow:ellipsis">Server address</span>
    </div>`,
  );
  const issues = await layoutIssues(page);
  expect(kinds(issues), formatIssues(issues)).toEqual(["overflowing"]);
  expect(issues[0]?.element).toContain("form.wide");
});

test("a label running under the select beside it is an overlap", async ({ page }) => {
  // The P1 verify finding: "Light theme" nowrap in a shrinking grid column,
  // the select painted over its end — the boxes themselves do not overlap.
  await show(
    page,
    `<div data-part="app">
      <div data-part="form.field" style="display:grid; grid-template-columns:minmax(0, 1fr) 140px; width:200px; column-gap:8px">
        <label data-part="form.label" style="white-space:nowrap">Light theme</label>
        <select data-part="form.select"><option>Frost</option></select>
      </div>
    </div>`,
  );
  const issues = await layoutIssues(page);
  expect(kinds(issues), formatIssues(issues)).toEqual(["overflowing", "overlap"]);
  const overlap = issues.find((i) => i.kind === "overlap");
  expect(`${overlap?.element ?? ""} / ${overlap?.other ?? ""}`).toContain("Light theme");
  expect(`${overlap?.element ?? ""} / ${overlap?.other ?? ""}`).toContain("form.select");
  expect(await layoutIssues(page, { skip: ["overflowing", "overlap"] })).toEqual([]);
});

test("text a scrolling pane hides is not overflowing", async ({ page }) => {
  await show(
    page,
    `<div data-part="app">
      <div data-part="pane" style="width:100px; overflow:auto"><div style="white-space:nowrap">A long line that scrolls inside its own pane</div></div>
      <button data-part="next">Next</button>
    </div>`,
  );
  const issues = await layoutIssues(page);
  expect(issues, formatIssues(issues)).toEqual([]);
});

test("overlapping controls are found within a window, not across windows", async ({ page }) => {
  await show(
    page,
    `<div data-part="app" style="position:relative">
      <div data-part="window" style="position:absolute; left:0; top:0; width:300px; height:200px">
        <button data-part="a" style="position:absolute; left:10px; top:10px; width:100px">Save</button>
        <button data-part="b" style="position:absolute; left:60px; top:12px; width:100px">Cancel</button>
      </div>
      <div data-part="window" style="position:absolute; left:50px; top:50px; width:300px; height:200px">
        <button data-part="c" style="position:absolute; left:0; top:0">Behind is fine</button>
      </div>
    </div>`,
  );
  const issues = await layoutIssues(page, { skip: ["page-scroll"] });
  expect(kinds(issues)).toEqual(["overlap"]);
  expect(issues[0]?.element).toContain('"Save"');
  expect(issues[0]?.other).toContain('"Cancel"');
});

test("content hidden by a scrolling pane does not count as an overlap", async ({ page }) => {
  await show(
    page,
    `<div data-part="app">
      <div data-part="pane" style="height:40px; overflow:auto"><div style="height:30px"></div><button data-part="hidden-below">Below the fold</button></div>
      <button data-part="next">Next</button>
    </div>`,
  );
  const issues = await layoutIssues(page);
  expect(issues, formatIssues(issues)).toEqual([]);
});

test("controls sticking out of the viewport are found", async ({ page }) => {
  await show(
    page,
    `<div data-part="app" style="position:relative">
      <button data-part="far" style="position:absolute; left:760px; width:120px">Off the edge</button>
    </div>`,
  );
  const issues = await layoutIssues(page);
  // The body hides its overflow, so the page is wider than the viewport too.
  expect(kinds(issues)).toEqual(["offscreen", "page-overflow"]);
});

test("misaligned label columns are found when a test names them", async ({ page }) => {
  await show(
    page,
    `<div data-part="app">
      <div data-part="row"><label data-part="field.label">Name</label></div>
      <div data-part="row" style="padding-left:12px"><label data-part="field.label">Address</label></div>
    </div>`,
  );
  expect(await layoutIssues(page)).toEqual([]);
  const issues = await layoutIssues(page, { aligned: ['[data-part="field.label"]'] });
  expect(kinds(issues)).toEqual(["misaligned"]);
});

test("ignored subtrees and scopes limit the check", async ({ page }) => {
  await show(
    page,
    `<div data-part="app">
      <section data-part="one"><span data-part="x" style="display:block; width:40px; white-space:nowrap; overflow:hidden">Clipped text here</span></section>
      <section data-part="two"><p data-part="y">Fine</p></section>
    </div>`,
  );
  expect(kinds(await layoutIssues(page))).toEqual(["clipped"]);
  expect(await layoutIssues(page, { ignore: ['[data-part="one"]'] })).toEqual([]);
  expect(await layoutIssues(page, { scope: '[data-part="two"]' })).toEqual([]);
});

test("the sweep visits every standard size and saves screenshots", async ({ page }) => {
  expect(STANDARD_SIZES).toHaveLength(8);
  await show(page, `<div data-part="app"><p data-part="text">Hello</p></div>`);
  const seen: string[] = [];
  const reports = await checkResponsive(page, "self-test", {
    prepare: (p, size) => {
      seen.push(`${String(size.width)}x${String(size.height)}`);
      expect(p.viewportSize()).toEqual(size);
      return Promise.resolve();
    },
  });
  expect(seen).toEqual([
    "1600x1000",
    "1600x700",
    "1280x1000",
    "1280x700",
    "1024x1000",
    "1024x700",
    "800x1000",
    "800x700",
  ]);
  for (const report of reports) {
    expect(report.issues).toEqual([]);
    expect(report.screenshot).toBe(screenshotPath("self-test", report.size));
    expect(existsSync(report.screenshot ?? "")).toBe(true);
  }
});

test("resizeWindow drags the bottom-right handle and stops at the min-size", async ({ page }) => {
  await show(
    page,
    `<div data-part="window" style="position:absolute; left:20px; top:20px; width:400px; height:300px; min-width:200px; min-height:150px">
      <div data-part="window.handle" data-edge="se" style="position:absolute; right:0; bottom:0; width:10px; height:10px"></div>
    </div>
    <script>
      const win = document.querySelector('[data-part="window"]');
      const handle = win.querySelector('[data-edge="se"]');
      let start = null;
      handle.addEventListener("pointerdown", (e) => { start = { x: e.clientX, y: e.clientY, w: win.offsetWidth, h: win.offsetHeight }; handle.setPointerCapture(e.pointerId); });
      handle.addEventListener("pointermove", (e) => { if (!start) return; win.style.width = Math.max(200, start.w + e.clientX - start.x) + "px"; win.style.height = Math.max(150, start.h + e.clientY - start.y) + "px"; });
      handle.addEventListener("pointerup", () => { start = null; });
    </script>`,
  );
  const frame = page.locator('[data-part="window"]');
  expect(await resizeWindow(page, frame, { width: 500, height: 400 })).toEqual({
    width: 500,
    height: 400,
  });
  expect(await resizeWindow(page, frame, { width: 1, height: 1 })).toEqual({
    width: 200,
    height: 150,
  });
});

test("visually hidden screen-reader text is not clipped text", async ({ page }) => {
  await show(
    page,
    `<div data-part="app">
      <button data-part="rail.item"><span data-part="rail.label" style="position:absolute; width:1px; height:1px; overflow:hidden; clip:rect(0 0 0 0); white-space:nowrap">Command palette</span>⌘</button>
    </div>`,
  );
  const issues = await layoutIssues(page);
  expect(issues, formatIssues(issues)).toEqual([]);
});

test("a control left at the browser's default width in a row with room is undersized", async ({
  page,
}) => {
  // The P1 composer finding: To sat in a block wrapper inside its grid row at
  // 20 characters while Subject spanned the row.
  await show(
    page,
    `<div data-part="app">
      <div data-part="composer.field" style="display:grid; grid-template-columns:80px minmax(0, 1fr); gap:8px; width:600px">
        <label data-part="composer.label">To</label>
        <div data-part="composer.address"><input data-part="composer.field.input" aria-label="To"></div>
        <label data-part="composer.label">Subject</label>
        <input data-part="composer.field.input" aria-label="Subject">
      </div>
      <div data-part="settings.row" style="display:flex; gap:8px; width:600px">
        <label data-part="form.label">Server</label>
        <input data-part="form.input" aria-label="Server">
      </div>
      <div data-part="form.field" style="display:grid; justify-items:start; width:600px">
        <label data-part="form.label">Theme</label>
        <select data-part="form.select" aria-label="Theme"><option>Frost</option></select>
      </div>
      <div data-part="form.field" style="display:block; width:600px">
        <textarea data-part="form.textarea" aria-label="Notes"></textarea>
      </div>
    </div>`,
  );
  const issues = await layoutIssues(page);
  expect(kinds(issues), formatIssues(issues)).toEqual([
    "undersized-control",
    "undersized-control",
    "undersized-control",
    "undersized-control",
  ]);
  const found = issues.map((i) => i.element).join("\n");
  for (const name of ["[To]", "[Server]", "[Theme]", "[Notes]"]) expect(found).toContain(name);
  expect(found).not.toContain("[Subject]");
});

test("controls that fill their row, grow or are marked small are not undersized", async ({
  page,
}) => {
  await show(
    page,
    `<div data-part="app">
      <div data-part="composer.address" style="width:600px"><input data-part="composer.field.input" aria-label="To" style="width:100%"></div>
      <div data-part="settings.row" style="display:flex; gap:8px; width:600px">
        <label data-part="form.label">Server</label>
        <input data-part="form.input" aria-label="Server" style="flex:1 1 auto; min-width:0">
        <input data-part="form.input" aria-label="Port" type="number" data-width="short" style="width:80px">
      </div>
      <div data-part="settings.row" style="display:flex; gap:8px; width:600px">
        <label data-part="form.label">Remind me</label>
        <input data-part="form.input" aria-label="Minutes" type="number" data-width="short" style="width:80px">
        <span data-part="form.hint">minutes before</span>
      </div>
      <div data-part="toolbar" style="display:flex; gap:8px; width:600px">
        <button data-part="toolbar.button">Reply</button>
        <input data-part="toolbar.search" aria-label="Search" data-width="content" style="width:160px">
      </div>
      <div data-part="pim.datetime" style="display:flex; gap:8px; width:600px">
        <span data-part="pim.datetime.date" data-width="medium" style="display:inline-flex"><input data-part="form.input" type="date" aria-label="Date"></span>
        <input data-part="form.input" type="time" aria-label="Time" data-width="short">
      </div>
      <div data-part="editor" contenteditable="true" aria-label="Body" style="display:block; width:600px; min-height:40px"><p>Text</p></div>
      <div data-part="form.field" style="display:grid; width:600px">
        <label data-part="form.label">Name</label>
        <input data-part="form.input" aria-label="Name">
        <select data-part="form.select" aria-label="Kind"><option>Work</option></select>
      </div>
    </div>`,
  );
  const issues = await layoutIssues(page);
  expect(issues, formatIssues(issues)).toEqual([]);
});

test("a control short of the edge its column siblings share is undersized", async ({ page }) => {
  await show(
    page,
    `<div data-part="app">
      <form data-part="form" aria-label="Account" style="width:420px">
        <div data-part="row"><input data-part="form.input" aria-label="Name" style="width:400px"></div>
        <div data-part="row"><input data-part="form.input" aria-label="Address" style="width:400px"></div>
        <div data-part="row"><select data-part="form.select" aria-label="Kind" style="width:370px"><option>IMAP</option></select></div>
        <div data-part="row"><input data-part="form.input" aria-label="Date" style="width:300px"><button data-part="form.button" style="width:96px">Pick</button></div>
      </form>
    </div>`,
  );
  const issues = await layoutIssues(page);
  expect(kinds(issues), formatIssues(issues)).toEqual(["undersized-control"]);
  expect(issues[0]?.element).toContain("[Kind]");
  expect(issues[0]?.detail).toContain("column");
  expect(await layoutIssues(page, { skip: ["undersized-control"] })).toEqual([]);
});
