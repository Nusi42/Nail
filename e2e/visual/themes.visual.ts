/**
 * The visual-regression matrix (plan M5a WP154): every screen of
 * `screens.ts` in every built-in theme × density, compared with the
 * reviewed baselines in `baselines/<platform>/<theme>-<density>/`.
 *
 * The variants come from the core's `ListThemes` (probed by the config,
 * `lib/matrix.ts`); a theme the core does not list, lists with a problem,
 * or cannot list at all (before M5a WP152: `unsupported` /
 * `m5a-not-ready`) is a skipped test whose reason says so. A missing
 * baseline fails as "new"; `npm run visual:update` writes it.
 */
import { SCREENS } from "./screens.ts";
import { parseFilter, planVariants } from "./lib/matrix.ts";
import {
  applyVariant,
  collectedProbe,
  compareWithBaseline,
  expectDensity,
  openAndShoot,
  recordError,
  test,
  writeEntry,
} from "./support.ts";

const variants = planVariants(collectedProbe(), {
  themes: parseFilter(process.env.NAIL_VISUAL_THEMES),
  densities: parseFilter(process.env.NAIL_VISUAL_DENSITIES),
});
const screens = SCREENS.filter((s) => {
  const only = parseFilter(process.env.NAIL_VISUAL_SCREENS);
  return !only || only.includes(s.id);
});

function recordSkip(variant: (typeof variants)[number], reason: string) {
  writeEntry({
    variant: variant.id,
    theme: variant.theme,
    density: variant.density,
    screen: null,
    status: "skipped",
    baseline: null,
    actual: null,
    diff: null,
    message: reason,
  });
}

for (const variant of variants) {
  const title = `${variant.theme} · ${variant.density}`;
  const reason = variant.skip;
  if (reason) {
    // No fixtures: a skipped variant starts no server.
    test(title, async ({}, testInfo) => {
      recordSkip(variant, reason);
      testInfo.annotations.push({ type: "skip-reason", description: reason });
      test.skip(true, reason);
    });
    continue;
  }
  test(title, async ({ page, nail }) => {
    const unavailable = await applyVariant(nail, variant);
    if (unavailable) {
      recordSkip(variant, unavailable);
      test.skip(true, unavailable);
    }
    const failures: string[] = [];
    for (const screen of screens) {
      await test.step(screen.id, async () => {
        try {
          const shot = await openAndShoot(page, nail.url, screen);
          await expectDensity(page, variant.density);
          const entry = compareWithBaseline(test.info(), variant, screen, shot);
          if (entry.status === "error") failures.push(`${screen.id}: ${entry.message}`);
        } catch (error) {
          // Recorded, and the remaining screens are still captured.
          failures.push(`${screen.id}: ${await recordError(page, variant, screen, error)}`);
        }
      });
    }
    if (failures.length > 0) throw new Error(failures.join("\n"));
  });
}
