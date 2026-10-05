/**
 * Fixtures and helpers of the visual-regression harness (plan M5a WP154):
 * a private `nail serve --seed demo-multi` per worker (themes are global
 * state of a server, so workers never share one), selecting a theme ×
 * density through the core (`SetTheme`, `SetThemePreferences`; contract
 * ADR 0102), a deterministic page (fixed clock, time zone, locale, colour
 * scheme, no animations, fonts settled) and capturing a screen into the
 * manifest the review page is built from.
 *
 * Output (screenshots, manifest, Playwright results, the review page) goes
 * to `<build dir>/visual/` (`NAIL_BUILD_DIR`, default the checkout's
 * gitignored `.nail-build`), never into tracked files; only reviewed
 * baselines live in `e2e/visual/baselines/`.
 */
import { existsSync, mkdirSync, writeFileSync } from "node:fs";
import path from "node:path";
import { expect, test as base, type Page, type TestInfo } from "@playwright/test";
import { control, startNail, type NailServer } from "../support/nail";
import { rpc } from "../support/remote";
import {
  captureStable,
  comparisonStatus,
  writeManifestEntry,
  type ManifestEntry,
} from "./lib/capture.ts";
import {
  CURRENT_THEME,
  preferencesFor,
  probeFromAnswer,
  unavailableReason,
  type AppearanceLike,
  type ThemeProbe,
  type Variant,
} from "./lib/matrix.ts";
import { actualDir, manifestDir, VIEWPORT } from "./lib/paths.ts";
import {
  ATTACHMENT_MESSAGE,
  CALENDAR_EVENTS,
  CONVERSATION_UNREAD,
  HTML_MESSAGE,
  MAIN_TIME,
  calendarEventIcs,
  loadMainWindow,
  type Screen,
} from "./screens.ts";

/** The theme probe the config made while collecting the tests. */
export function collectedProbe(): ThemeProbe {
  const raw = process.env.NAIL_VISUAL_PROBE;
  if (!raw) return { ok: false, reason: "the theme probe did not run" };
  return JSON.parse(raw) as ThemeProbe;
}

/** A private demo-multi server per worker, with the calendar screen's events synced. */
export const test = base.extend<object, { nail: NailServer }>({
  nail: [
    // eslint-disable-next-line no-empty-pattern -- Playwright requires a destructuring pattern for fixtures without dependencies
    async ({}, use) => {
      const server = await startNail({ seed: "demo-multi", locale: "en" });
      try {
        await seedCalendar(server);
        await use(server);
      } finally {
        await server.stop();
      }
    },
    { scope: "worker", timeout: 120_000 },
  ],
});

export { expect };

/** Puts the fixed events on the mock CalDAV server and waits until the core has them. */
async function seedCalendar(server: NailServer): Promise<void> {
  for (const event of CALENDAR_EVENTS) {
    await control(server, "dav-put", {
      collection: "personal",
      name: `${event.uid}.ics`,
      data: calendarEventIcs(event),
    });
  }
  const deadline = Date.now() + 60_000;
  for (;;) {
    await rpc(server, { type: "SyncPimSources", source_id: null });
    const answer = await rpc(server, {
      type: "ListEventInstances",
      from: "2026-08-09T00:00:00Z",
      to: "2026-08-16T00:00:00Z",
      calendar_ids: null,
      tz: "UTC",
    });
    const instances = (answer.instances as unknown[] | undefined) ?? [];
    if (instances.length >= CALENDAR_EVENTS.length) return;
    if (Date.now() > deadline) {
      throw new Error(`the calendar events did not sync: ${JSON.stringify(answer)}`);
    }
    await new Promise((resolve) => setTimeout(resolve, 500));
  }
}

/** `ListThemes` on the worker's server. */
export async function listThemes(server: NailServer): Promise<ThemeProbe> {
  return probeFromAnswer(await rpc(server, { type: "ListThemes" }));
}

/**
 * Selects the variant's theme and density on the server (Follow Windows
 * off, no overrides). Answers the reason when the theme turned out to be
 * unavailable on this server, else `null`.
 */
export async function applyVariant(server: NailServer, variant: Variant): Promise<string | null> {
  const probe = await listThemes(server);
  const reason = unavailableReason(probe, variant.theme);
  if (reason) return reason;
  if (!probe.ok) {
    // The current theme as the server started (comfortable); nothing to select.
    return variant.theme === CURRENT_THEME && variant.density === "comfortable"
      ? null
      : `ListThemes failed: ${probe.reason}`;
  }
  if (variant.theme !== CURRENT_THEME) {
    const set = await rpc(server, { type: "SetTheme", theme_id: variant.theme });
    expect(set.type, `SetTheme ${variant.theme}: ${JSON.stringify(set)}`).toBe("Themes");
  }
  const prefs = await rpc(server, {
    type: "SetThemePreferences",
    preferences: preferencesFor(probe.appearance, variant.density),
  });
  expect(prefs.type, `SetThemePreferences: ${JSON.stringify(prefs)}`).toBe("Themes");
  const appearance = prefs.appearance as AppearanceLike;
  if (variant.theme !== CURRENT_THEME) {
    expect(appearance.active, "the active theme after selecting it").toBe(variant.theme);
  }
  expect(appearance.settings.density, "the density after selecting it").toBe(variant.density);
  return null;
}

/** Fixed clock and media features; called before each screen's page load. */
export async function preparePage(page: Page, time: Date = MAIN_TIME): Promise<void> {
  await page.clock.setFixedTime(time);
  await page.emulateMedia({
    colorScheme: "light",
    reducedMotion: "reduce",
    forcedColors: "none",
    contrast: "no-preference",
  });
}

/**
 * Marks the messages these searches find unread again. The screens that
 * open them (HTML, attachments, the conversation's unread card) mark them read, and the worker's server is
 * shared by every variant: without this, whether the main window shows
 * them read depended on which variants ran before on that worker.
 */
async function markUnread(server: NailServer, queries: readonly string[]): Promise<void> {
  for (const query of queries) {
    const found = await rpc(server, {
      type: "Search",
      query: {
        text: query,
        account_id: null,
        folder_id: null,
        offset: 0,
        limit: 10,
        sort: "date",
      },
    });
    const hits = (found.hits ?? []) as { message: { id: string; flags: { seen: boolean } } }[];
    const ids = hits.filter((h) => h.message.flags.seen).map((h) => h.message.id);
    if (ids.length === 0) continue;
    const answer = await rpc(server, {
      type: "UpdateFlags",
      message_ids: ids,
      change: {
        seen: false,
        flagged: null,
        answered: null,
        draft: null,
        add_tags: [],
        remove_tags: [],
      },
    });
    expect(answer.type, `UpdateFlags: ${JSON.stringify(answer)}`).not.toBe("Error");
  }
}

/**
 * Waits until the UI shows `density` (`data-nail-density` on the root;
 * absent = comfortable, ADR 0102), so a capture is never mislabelled.
 */
export async function expectDensity(page: Page, density: string): Promise<void> {
  await expect
    .poll(
      () => page.evaluate(() => document.documentElement.dataset.nailDensity ?? "comfortable"),
      {
        message: `the UI applies density ${density} (data-nail-density on the root)`,
      },
    )
    .toBe(density);
}

/** Fonts loaded, pointer parked where it hovers nothing interesting. */
async function settle(page: Page): Promise<void> {
  await page.evaluate(async () => {
    await document.fonts.ready;
  });
  const size = page.viewportSize() ?? VIEWPORT;
  await page.mouse.move(size.width - 2, size.height - 2);
}

export interface Shot {
  png: Buffer;
  stable: boolean;
  attempts: number;
}

/** Opens `screen` on a freshly loaded main window and captures it once it settled. */
export async function openAndShoot(page: Page, server: NailServer, screen: Screen): Promise<Shot> {
  // Most screens use the harness's window; a few check a narrower one.
  await page.setViewportSize(screen.viewport ?? VIEWPORT);
  await preparePage(page, screen.time);
  // The app reopens the last view (M5b UI state): every screen starts on
  // Mail. Leave the previous screen first, or its view host saves its view
  // again when it hears of the reset.
  await page.goto("about:blank");
  const reset = await rpc(server, { type: "SetUiState", key: "shell.view", value: null });
  expect(reset.type, `SetUiState: ${JSON.stringify(reset)}`).toBe("UiState");
  await markUnread(server, [
    `subject:"${HTML_MESSAGE}"`,
    `subject:"${ATTACHMENT_MESSAGE}"`,
    CONVERSATION_UNREAD,
  ]);
  await loadMainWindow(page, server.url);
  await screen.open(page);
  await settle(page);
  const shot = await captureStable(() =>
    page.screenshot({
      animations: "disabled",
      caret: "hide",
      scale: "css",
      mask: screen.mask?.(page) ?? [],
    }),
  );
  await screen.close?.(page);
  return shot;
}

/** Saves a screenshot under the actual directory; answers its path. */
export function saveActual(variant: string, screen: string, png: Buffer): string {
  const file = path.join(actualDir(), variant, `${screen}.png`);
  mkdirSync(path.dirname(file), { recursive: true });
  writeFileSync(file, png);
  return file;
}

const ansi = new RegExp(`${String.fromCharCode(27)}\\[[0-9;]*m`, "g");
const plain = (s: string | undefined) => (s ?? "").replace(ansi, "").trim();

/**
 * Compares a capture with its baseline (`expect.soft(...).toMatchSnapshot`
 * with the config's tolerance; `--update-snapshots` writes baselines) and
 * records the outcome. Answers the manifest entry.
 */
export function compareWithBaseline(
  testInfo: TestInfo,
  variant: Variant,
  screen: Screen,
  shot: Shot,
): ManifestEntry {
  const actual = saveActual(variant.id, screen.id, shot.png);
  const name = [variant.id, `${screen.id}.png`];
  const baseline = testInfo.snapshotPath(...name);
  const baselineExisted = existsSync(baseline);
  const errorsBefore = testInfo.errors.length;
  const attachmentsBefore = testInfo.attachments.length;
  expect.soft(shot.png, `${variant.id} › ${screen.id}`).toMatchSnapshot(name);
  const newErrors = testInfo.errors.slice(errorsBefore);
  const matched = newErrors.length === 0;
  const diff =
    testInfo.attachments
      .slice(attachmentsBefore)
      .find((a) => a.path && a.name.endsWith("-diff.png"))?.path ?? null;
  const updating = testInfo.config.updateSnapshots !== "none";
  let message: string | null = matched ? null : plain(newErrors[0]?.message);
  if (matched && updating && !baselineExisted) message = "baseline written";
  if (!shot.stable) {
    message = `the screen did not settle within ${String(shot.attempts)} screenshots; ${message ?? ""}`;
  }
  return writeEntry({
    variant: variant.id,
    theme: variant.theme,
    density: variant.density,
    screen: screen.id,
    status: shot.stable ? comparisonStatus(baselineExisted, matched) : "error",
    baseline: existsSync(baseline) ? baseline : null,
    actual,
    diff,
    message,
  });
}

/**
 * Records a screen that could not be opened (with whatever the page shows,
 * best effort); answers the message.
 */
export async function recordError(
  page: Page,
  variant: { id: string; theme: string; density: string },
  screen: Screen,
  error: unknown,
): Promise<string> {
  const message = plain(error instanceof Error ? error.message : String(error));
  let actual: string | null = null;
  try {
    actual = saveActual(variant.id, screen.id, await page.screenshot({ animations: "disabled" }));
  } catch {
    // The page may be gone; the message is what matters.
  }
  writeEntry({
    variant: variant.id,
    theme: variant.theme,
    density: variant.density,
    screen: screen.id,
    status: "error",
    baseline: null,
    actual,
    diff: null,
    message,
  });
  return message;
}

/** Records a manifest entry. */
export function writeEntry(entry: ManifestEntry): ManifestEntry {
  writeManifestEntry(manifestDir(), entry);
  return entry;
}
