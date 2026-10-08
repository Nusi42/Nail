/** axe in the end-to-end tests (`@axe-core/playwright`). */
import AxeBuilder from "@axe-core/playwright";
import type { Page } from "@playwright/test";

/**
 * Every axe violation on the page (or in the part `include` selects),
 * whatever its impact. The composer's preview frame is left out: it shows
 * the outgoing mail in a sandbox without scripts that axe cannot enter.
 */
export async function axeViolations(page: Page, include?: string) {
  const builder = new AxeBuilder({ page }).exclude('[data-part="composer.preview.frame"]');
  const results = await (include === undefined ? builder : builder.include(include)).analyze();
  return results.violations.map((v) => ({
    id: v.id,
    impact: v.impact,
    help: v.help,
    targets: v.nodes.map((n) => n.target.join(" ")),
  }));
}
