/**
 * Test support for the window layer's close guard (R6, ADR 0239): the app
 * installs it once (`features/windows`, `CloseGuardEffect`); a test that
 * renders windows without the app installs it on its env for the test.
 */
import { act } from "@testing-library/react";
import { installCloseGuard } from "../layout/closeGuard";
import type { Env } from "./harness";

/** Installs the close guard on `env`'s UI store until the test finishes. */
export function withCloseGuard(env: Env): void {
  let uninstall: (() => void) | null = null;
  act(() => {
    uninstall = installCloseGuard(env.store);
  });
  onTestFinished(() => uninstall?.());
}
