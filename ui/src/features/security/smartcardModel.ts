/**
 * Smartcard checks the UI makes before asking the core (ports of
 * `nail_api::smartcard`, ADR 0094, pinned by
 * `crates/nail-api/tests/ts_vectors.rs`; the constants are generated): the
 * settings' issues, the PIN cache limit, a typed PIN's plausibility.
 */
import {
  MAX_PIN_CACHE_MINUTES,
  SMARTCARD_SETTINGS_ISSUE_MESSAGE_IDS,
  MAX_PIN_CHARS,
  MIN_PIN_CHARS,
  type SmartcardSettings,
} from "../../api/generated";
import { charCount } from "../../text/chars";
export {
  MAX_PIN_CACHE_MINUTES,
  MAX_PIN_CHARS,
  MIN_PIN_CHARS,
  PIN_PROMPT_TIMEOUT_SECS,
} from "../../api/generated";

/** `SmartcardSettings::issues` (the first one), as a Fluent id. */
export function smartcardSettingsIssue(settings: SmartcardSettings): string | null {
  const absolute = (p: string) => /^([a-zA-Z]:[\\/]|\\\\|\/)/.test(p);
  if (settings.modules.some((m) => !absolute(m.path))) {
    return SMARTCARD_SETTINGS_ISSUE_MESSAGE_IDS["relative-path"];
  }
  const paths = settings.modules.map((m) => m.path.toLowerCase());
  if (new Set(paths).size !== paths.length) {
    return SMARTCARD_SETTINGS_ISSUE_MESSAGE_IDS["duplicate-module"];
  }
  if (settings.pin_cache_minutes > MAX_PIN_CACHE_MINUTES) {
    return SMARTCARD_SETTINGS_ISSUE_MESSAGE_IDS["cache-too-long"];
  }
  return null;
}

/** `pin_is_plausible`: 4–64 characters, no control characters. */
export function pinIsPlausible(pin: string): boolean {
  const chars = charCount(pin);
  return chars >= MIN_PIN_CHARS && chars <= MAX_PIN_CHARS && !/\p{Cc}/u.test(pin);
}
