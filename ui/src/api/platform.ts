/**
 * Platform features (ADR 0202), pure: the hooks are in `state/platform.ts`.
 */
import type { PlatformFeature, PlatformInfo } from "./generated";

/** Whether `platform` offers `feature` (`false` while it is unknown). */
export function hasFeature(platform: PlatformInfo | undefined, feature: PlatformFeature): boolean {
  return platform?.features.includes(feature) ?? false;
}
