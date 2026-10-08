/**
 * Fluent message ids of this feature for the catalog coverage test
 * (`../../i18n/coverage.test.ts`): literal ids in the sources starting with
 * one of `prefixes` must exist in `locales/en` (`connlog.ftl`; the
 * contract's ids are in `core-connlog.ftl`); ids built at runtime are listed
 * by `runtimeIds`.
 */
import {
  ERROR_STEPS,
  WIRE_DIRECTIONS,
  WIRE_LEVELS,
  WIRE_PROTOCOLS,
  directionId,
  levelId,
  protocolId,
  stepId,
} from "./model";
import { APP_LOG_KIND, CONNECTION_LOG_KIND, ERROR_DETAILS_KIND } from "./names";

/** Prefixes of message ids written literally in this feature's sources. */
export const prefixes: readonly string[] = ["connlog-", "error-step-"];

/** Literals that look like message ids but are not: a `ConnectionLogScope` kind. */
export const notMessages: readonly string[] = ["pim-source"];

/** Message ids built at runtime. */
export function runtimeIds(): string[] {
  return [
    ...WIRE_PROTOCOLS.map(protocolId),
    ...WIRE_LEVELS.map(levelId),
    ...WIRE_LEVELS.map((l) => `connlog-viewer-level-from-${l}`),
    ...WIRE_DIRECTIONS.map(directionId),
    ...ERROR_STEPS.map(stepId),
    ...[CONNECTION_LOG_KIND, ERROR_DETAILS_KIND, APP_LOG_KIND].map((k) => `window-${k}`),
    "sync-skipped-messages",
  ];
}
