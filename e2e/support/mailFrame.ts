import { expect, type FrameLocator } from "@playwright/test";
import { FRAME_SCRIPT } from "../../ui/src/api/generated/constants";

/**
 * Asserts that the mail document in `frame` holds no script of the mail's
 * own: its one script is the reader's frame script (ADR 0212).
 */
export async function expectOnlyFrameScript(frame: FrameLocator): Promise<void> {
  const scripts = frame.locator("script");
  await expect(scripts).toHaveCount(1);
  expect(await scripts.textContent()).toBe(FRAME_SCRIPT);
}
