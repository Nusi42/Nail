/**
 * Self-check of the visual-regression harness (plan M5a WP154), with
 * whatever theme the server starts in and no baselines: every screen of
 * `screens.ts` opens, settles (two consecutive screenshots identical) and
 * is reproducible — captured twice from a fresh page load, the captures
 * may differ in no more pixels than the baseline tolerance allows. A screen that fails here would make every
 * baseline comparison flaky (a clock, an animation, a late font, data that
 * depends on the real date). The captures appear on the review page as
 * the variant `current`.
 */
import { SCREENS } from "./screens.ts";
import { parseFilter } from "./lib/matrix.ts";
import { tolerance } from "./lib/paths.ts";
import { diffPngs } from "./lib/png.ts";
import {
  expect,
  listThemes,
  openAndShoot,
  recordError,
  saveActual,
  test,
  writeEntry,
} from "./support.ts";

const { maxDiffPixelRatio } = tolerance();
const only = parseFilter(process.env.NAIL_VISUAL_SCREENS);
const screens = SCREENS.filter((s) => !only || only.includes(s.id));

test("every screen opens, settles and is reproducible (current theme)", async ({
  page,
  nail,
}, testInfo) => {
  const probe = await listThemes(nail);
  const variant = {
    id: "current",
    theme: probe.ok ? probe.appearance.active : "default",
    density: probe.ok ? probe.appearance.settings.density : "comfortable",
  };
  const failures: string[] = [];
  for (const screen of screens) {
    await test.step(screen.id, async () => {
      try {
        const first = await openAndShoot(page, nail.url, screen);
        const second = await openAndShoot(page, nail.url, screen);
        const actual = saveActual(variant.id, screen.id, second.png);
        const problems: string[] = [];
        if (!first.stable || !second.stable) {
          problems.push(
            `did not settle within ${Math.max(first.attempts, second.attempts)} screenshots`,
          );
        }
        const diff = diffPngs(first.png, second.png);
        if (diff.ratio > maxDiffPixelRatio) {
          problems.push(
            `two captures from fresh page loads differ in ${diff.pixels} pixels ` +
              `(box ${JSON.stringify(diff.box)}, more than the tolerance ${maxDiffPixelRatio})`,
          );
          for (const [which, shot] of [
            ["first", first],
            ["second", second],
          ] as const) {
            await testInfo.attach(`${screen.id}-${which}.png`, {
              path: saveActual(variant.id, `${screen.id}-${which}`, shot.png),
              contentType: "image/png",
            });
          }
        }
        writeEntry({
          variant: variant.id,
          theme: variant.theme,
          density: variant.density,
          screen: screen.id,
          status: problems.length === 0 ? "captured" : "error",
          baseline: null,
          actual,
          diff: null,
          message: problems.length === 0 ? null : problems.join("; "),
        });
        if (problems.length > 0) failures.push(`${screen.id}: ${problems.join("; ")}`);
      } catch (error) {
        failures.push(`${screen.id}: ${await recordError(page, variant, screen, error)}`);
      }
    });
  }
  expect(failures, "screens that are not reproducible").toEqual([]);
});
