/**
 * The platform the core runs on (ADR 0202): its system and the features
 * only some systems have. It arrives with `GetConfig`; the UI hides what
 * the system cannot do (a Windows-only setting on macOS or Linux) instead
 * of showing it and failing.
 */
import type { QueryClient } from "@tanstack/react-query";
import type { PlatformFeature, PlatformInfo } from "../api/generated";
import { hasFeature } from "../api/platform";
import { queryKeys, useConfig, type AppConfig } from "./queries";

export { hasFeature };

/** The loaded platform, if any (for actions, outside React). */
export function loadedPlatform(queryClient: QueryClient): PlatformInfo | undefined {
  return queryClient.getQueryData<AppConfig>(queryKeys.config)?.platform;
}

/** The platform, once the config is loaded. */
export function usePlatform(): PlatformInfo | undefined {
  return useConfig().data?.platform;
}

/** Whether the platform offers `feature`. */
export function usePlatformFeature(feature: PlatformFeature): boolean {
  return hasFeature(usePlatform(), feature);
}
