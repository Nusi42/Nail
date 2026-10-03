/**
 * axe-core accessibility checks for the jsdom suites.
 */
import { act } from "@testing-library/react";
import axe from "axe-core";

/**
 * The axe violations under `root`, one line each (empty = none).
 *
 * The check runs inside `act`: what the app still does meanwhile (a query
 * answering, a debounce firing) is committed after the check instead of
 * changing the DOM while axe walks it, and is not an update outside act.
 */
export async function violations(root: Element): Promise<string[]> {
  const result = await act(() =>
    axe.run(root, {
      iframes: false,
      rules: {
        // jsdom has no layout: contrast is checked in the browser (Playwright).
        "color-contrast": { enabled: false },
        // Single widgets and windows are rendered outside the page's landmarks.
        region: { enabled: false },
      },
    }),
  );
  return result.violations.map(
    (v) =>
      `${v.id}: ${v.help} — ${v.nodes
        .slice(0, 3)
        .map((n) => n.target.join(" "))
        .join(" | ")}`,
  );
}
