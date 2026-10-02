/**
 * Smartcard checks the UI makes before asking the core (mirrors
 * `nail_api::smartcard`, ADR 0094): the settings' issues, the PIN cache
 * limit, a typed PIN's plausibility.
 */
import type { PinPromptCloseReason, PinPurpose, SmartcardSettings } from "../../api/generated";

/** `MAX_PIN_CACHE_MINUTES`. */
export const MAX_PIN_CACHE_MINUTES = 480;
/** `PIN_PROMPT_TIMEOUT_SECS`. */
export const PIN_PROMPT_TIMEOUT_SECS = 120;
/** `MIN_PIN_CHARS` / `MAX_PIN_CHARS`. */
export const MIN_PIN_CHARS = 4;
export const MAX_PIN_CHARS = 64;

export const PIN_PURPOSES: readonly PinPurpose[] = ["login", "sign", "decrypt"];
export const PIN_CLOSE_REASONS: readonly PinPromptCloseReason[] = [
  "answered",
  "timed-out",
  "token-removed",
  "cancelled",
];

/** `SmartcardSettings::issues` (the first one), as a Fluent id. */
export function smartcardSettingsIssue(settings: SmartcardSettings): string | null {
  const absolute = (p: string) => /^([a-zA-Z]:[\\/]|\\\\|\/)/.test(p);
  if (settings.modules.some((m) => !absolute(m.path))) {
    return "smartcard-settings-issue-relative-path";
  }
  const paths = settings.modules.map((m) => m.path.toLowerCase());
  if (new Set(paths).size !== paths.length) return "smartcard-settings-issue-duplicate-module";
  if (settings.pin_cache_minutes > MAX_PIN_CACHE_MINUTES) {
    return "smartcard-settings-issue-cache-too-long";
  }
  return null;
}

/** `pin_is_plausible`: 4–64 characters, no control characters. */
export function pinIsPlausible(pin: string): boolean {
  const chars = [...pin].length;
  return chars >= MIN_PIN_CHARS && chars <= MAX_PIN_CHARS && !/\p{Cc}/u.test(pin);
}
