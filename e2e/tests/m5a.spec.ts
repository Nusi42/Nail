import { readFileSync, mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test, type Locator, type Page } from "@playwright/test";
import { control, startNail, type NailServer } from "../support/nail";
import { openSettingsTab, showView, viewRegion } from "../support/views";

/**
 * Wave M5a (ADRs 0102–0107) against `nail serve --seed demo-multi`: the
 * five built-in themes picked in Settings → Appearance and written to
 * `theme.kdl`, Follow Windows driven by the `system-appearance` test
 * control (light → Graphite, contrast theme → High contrast), the Compact
 * density (tag line hidden), the accent override, a user theme from
 * `themes/` picked up while running, and an attachment saved from the
 * reader (a download in the browser).
 */

let server: NailServer;

test.beforeEach(async () => {
  server = await startNail({ seed: "demo-multi" });
});

test.afterEach(async () => {
  await server.stop();
});

/** Accent colours of the built-in themes (defaults/themes/*.kdl). */
const ACCENT = {
  frost: "#3b5bdb",
  sage: "#2c7153",
  graphite: "#7c9cff",
  nocturne: "#45c9bb",
  "high-contrast": "#ffff00",
} as const;

async function open(page: Page) {
  await page.goto(server.url);
  await expect(page.locator('[data-part="message-list.row"]').first()).toBeVisible();
}

/** The accent the page renders now (the compiled theme's token on the root). */
function accent(page: Page): Promise<string> {
  return page.evaluate(() =>
    getComputedStyle(document.documentElement)
      .getPropertyValue("--nail-color-accent")
      .trim()
      .toLowerCase(),
  );
}

async function expectAccent(page: Page, value: string) {
  await expect.poll(() => accent(page)).toBe(value);
}

async function openAppearance(page: Page): Promise<Locator> {
  await showView(page, "Settings");
  const settings = viewRegion(page, "Settings");
  await openSettingsTab(page, "Appearance");
  const region = settings.getByRole("region", { name: "Appearance" });
  await expect(region.getByRole("radiogroup", { name: "Themes" })).toBeVisible();
  return region;
}

const themeFile = () => {
  try {
    return readFileSync(path.join(server.configDir, "theme.kdl"), "utf8");
  } catch {
    return "";
  }
};

test("themes are picked in Settings → Appearance and kept in theme.kdl", async ({ page }) => {
  await open(page);
  // First run: Follow Windows with Frost on a light OS, Comfortable.
  await expectAccent(page, ACCENT.frost);
  await expect(page.locator("html")).not.toHaveAttribute("data-nail-density", "compact");

  const appearance = await openAppearance(page);
  const gallery = appearance.getByRole("radiogroup", { name: "Themes" });
  await expect(gallery.getByRole("radio")).toHaveText([
    /Frost/,
    /Sage/,
    /Graphite/,
    /Nocturne/,
    /High contrast/,
  ]);
  const follow = appearance.getByRole("switch", { name: "Follow Windows" });
  await expect(follow).toHaveAttribute("aria-checked", "true");

  // Picking a theme fixes it and switches Follow Windows off.
  for (const [name, id] of [
    ["Nocturne", "nocturne"],
    ["Sage", "sage"],
    ["Graphite", "graphite"],
  ] as const) {
    await gallery.getByRole("radio", { name: new RegExp(name) }).click();
    await expectAccent(page, ACCENT[id]);
    await expect(gallery.getByRole("radio", { name: new RegExp(name) })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    await expect.poll(themeFile).toContain(`extends="${id}"`);
  }
  await expect(follow).toHaveAttribute("aria-checked", "false");
  // A fixed `extends` without an `appearance` line does not follow Windows.
  expect(themeFile()).not.toContain("follow-system=#true");

  // The accent override applies on top of the theme.
  await appearance.getByRole("textbox", { name: "Accent colour", exact: true }).fill("#ff0066");
  await appearance.getByRole("button", { name: "Apply" }).click();
  await expectAccent(page, "#ff0066");
  await expect.poll(themeFile).toContain('color "accent" "#ff0066"');
  await appearance.getByRole("button", { name: "Use theme defaults" }).click();
  await expectAccent(page, ACCENT.graphite);
  await expect.poll(themeFile).not.toContain('color "accent"');
});

test("High contrast keeps its own accent; the override applies again afterwards", async ({
  page,
}) => {
  // ADR 0161: the accent override is ignored while a high-contrast theme is active.
  await open(page);
  const appearance = await openAppearance(page);
  const field = appearance.getByRole("textbox", { name: "Accent colour", exact: true });
  const picker = appearance.getByLabel("Pick the accent colour", { exact: true });
  const noteText = "High contrast uses its own colours.";
  const note = appearance.getByText(noteText, { exact: true });
  await field.fill("#ff0066");
  await appearance.getByRole("button", { name: "Apply" }).click();
  await expectAccent(page, "#ff0066");
  await expect.poll(themeFile).toContain('color "accent" "#ff0066"');
  await expect(note).toBeHidden();
  await expect(field).toBeEnabled();

  // High contrast picked: its own accent, the control off with the note.
  const gallery = appearance.getByRole("radiogroup", { name: "Themes" });
  await gallery.getByRole("radio", { name: /High contrast/ }).click();
  await expect.poll(themeFile).toContain('extends="high-contrast"');
  await expectAccent(page, ACCENT["high-contrast"]);
  await expect(note).toBeVisible();
  await expect(field).toBeDisabled();
  await expect(picker).toBeDisabled();
  await expect(field).toHaveAccessibleDescription(noteText);
  // The stored value is kept and shown.
  await expect(field).toHaveValue("#ff0066");
  expect(themeFile()).toContain('color "accent" "#ff0066"');

  // Another theme: the override applies again.
  await gallery.getByRole("radio", { name: /Sage/ }).click();
  await expectAccent(page, "#ff0066");
  await expect(note).toBeHidden();
  await expect(field).toBeEnabled();

  // Follow Windows choosing High contrast does the same.
  await appearance.getByRole("switch", { name: "Follow Windows" }).click();
  await expect.poll(themeFile).toContain("follow-system=#true");
  await control(server, "system-appearance", { scheme: "dark", high_contrast: true });
  await expectAccent(page, ACCENT["high-contrast"]);
  await expect(note).toBeVisible();
  await expect(field).toBeDisabled();
  await control(server, "system-appearance", { scheme: "light" });
  await expectAccent(page, "#ff0066");
  await expect(note).toBeHidden();
});

test("Compact density applies to every theme and hides the tag line", async ({ page }) => {
  await open(page);
  const tagLines = page.locator('[data-part="message-list.row.tag-line"]');
  await expect(tagLines.first()).toBeVisible();
  const rowHeight = async () =>
    (await page.locator('[data-part="message-list.row"]').first().boundingBox())?.height ?? 0;
  const comfortable = await rowHeight();

  const appearance = await openAppearance(page);
  await appearance
    .getByRole("radiogroup", { name: "Density" })
    .getByRole("radio", { name: "Compact" })
    .click();
  await expect(page.locator("html")).toHaveAttribute("data-nail-density", "compact");
  await expect.poll(themeFile).toContain('density="compact"');
  // Back to Mail to see the rows (M5b: Settings is a view of its own).
  await showView(page, "Mail");
  await expect(tagLines.first()).toBeHidden();
  expect(await rowHeight()).toBeLessThan(comfortable);
  await openAppearance(page);

  // Still compact in another theme.
  await appearance
    .getByRole("radiogroup", { name: "Themes" })
    .getByRole("radio", { name: /Nocturne/ })
    .click();
  await expectAccent(page, ACCENT.nocturne);
  await expect(page.locator("html")).toHaveAttribute("data-nail-density", "compact");
});

test("Follow Windows switches with the OS, including contrast themes", async ({ page }) => {
  await open(page);
  await expectAccent(page, ACCENT.frost);
  await control(server, "system-appearance", { scheme: "dark" });
  await expectAccent(page, ACCENT.graphite);
  await control(server, "system-appearance", { scheme: "dark", high_contrast: true });
  await expectAccent(page, ACCENT["high-contrast"]);
  await expect(page.locator("html")).toHaveAttribute("data-nail-mode", "high-contrast");
  await control(server, "system-appearance", { scheme: "light" });
  await expectAccent(page, ACCENT.frost);

  // Another pair: Sage / Nocturne.
  const appearance = await openAppearance(page);
  await appearance.getByRole("combobox", { name: "Light theme" }).selectOption("sage");
  await expectAccent(page, ACCENT.sage);
  await appearance.getByRole("combobox", { name: "Dark theme" }).selectOption("nocturne");
  await control(server, "system-appearance", { scheme: "dark" });
  await expectAccent(page, ACCENT.nocturne);
  await expect.poll(themeFile).toContain('light="sage"');
  await expect.poll(themeFile).toContain('dark="nocturne"');
  // Nothing was written by the OS switches themselves.
  expect(themeFile()).toContain("follow-system=#true");
});

test("a theme file added to themes/ is listed at once and can be used", async ({ page }) => {
  await open(page);
  const appearance = await openAppearance(page);
  const gallery = appearance.getByRole("radiogroup", { name: "Themes" });
  await expect(gallery.getByRole("radio")).toHaveCount(5);
  mkdirSync(path.join(server.configDir, "themes"), { recursive: true });
  writeFileSync(
    path.join(server.configDir, "themes", "ocean.kdl"),
    'theme "Ocean" extends="graphite" {\n    tokens {\n        color "accent" "#4fb3d9"\n    }\n}\n',
  );
  const ocean = gallery.getByRole("radio", { name: /Ocean/ });
  await expect(ocean).toBeVisible({ timeout: 15_000 });
  await expect(ocean).toContainText("your theme");
  await ocean.click();
  await expectAccent(page, "#4fb3d9");
  await expect.poll(themeFile).toContain('extends="ocean"');

  // A broken user theme is listed with its problem and cannot be picked.
  writeFileSync(
    path.join(server.configDir, "themes", "broken.kdl"),
    'theme "Broken" extends="nowhere" {\n}\n',
  );
  const broken = gallery.getByRole("radio", { name: /Broken/ });
  await expect(broken).toBeDisabled({ timeout: 15_000 });
  await expect(broken).toContainText("This theme file has an error");
  await expect(broken).toContainText("nowhere");
});

test("an attachment is downloaded from its card in the browser", async ({ page }) => {
  await open(page);
  // The demo invoice carries a PDF and a CSV file.
  await page
    .locator('[data-part="message-list.row"]', { hasText: "Your invoice for September 2026" })
    .first()
    .click();
  const cards = page.locator('[data-part="attachment-card"]');
  await expect(cards).toHaveCount(2);
  await expect(cards.nth(0).locator('[data-part="attachment-card.type"]')).toHaveText("PDF");
  const card = cards.filter({ hasText: "usage.csv" });
  await expect(card.locator('[data-part="attachment-card.type"]')).toHaveText("CSV");
  const download = page.waitForEvent("download");
  await card.getByRole("button", { name: "Save usage.csv" }).click();
  const file = await download;
  expect(file.suggestedFilename()).toBe("usage.csv");
  const saved = readFileSync(await file.path(), "utf8").split(/\r?\n/);
  expect(saved.slice(0, 2)).toEqual(["date,service,amount", "2026-09-01,vm-small,6.00"]);
  await expect(card.getByRole("status")).toHaveText("Downloaded.");
});
