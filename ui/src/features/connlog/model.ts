/**
 * Pure logic of the connection log UI (ADR 0190): scopes ↔ window ids,
 * merging fetched pages into what the viewer holds, filters and the
 * Fluent ids of the contract's enums. The line format of copy / export is
 * Rust's (`ConnectionLogEntry::line`, fetched through `ExportConnectionLog`),
 * never rebuilt here.
 */
import {
  ERROR_STEP_VALUES,
  WIRE_DIRECTION_VALUES,
  WIRE_LEVEL_VALUES,
  WIRE_PROTOCOL_VALUES,
  type ConnectionLog,
  type ConnectionLogEntry,
  type ConnectionLogScope,
  type ErrorStep,
  type WireDirection,
  type WireLevel,
  type WireProtocol,
} from "../../api/generated";
import { isWindowInstance, parseWindowId, windowId } from "../../state/windows";
import { CONNECTION_LOG_KIND } from "./names";

/** Every value of the contract's enums, in the contract's order (generated). */
export const WIRE_PROTOCOLS: readonly WireProtocol[] = WIRE_PROTOCOL_VALUES;
export const WIRE_LEVELS: readonly WireLevel[] = WIRE_LEVEL_VALUES;
export const WIRE_DIRECTIONS: readonly WireDirection[] = WIRE_DIRECTION_VALUES;
export const ERROR_STEPS: readonly ErrorStep[] = ERROR_STEP_VALUES;

/** Fluent id of a protocol's name (`core-connlog.ftl`). */
export function protocolId(protocol: WireProtocol): `connlog-protocol-${WireProtocol}` {
  return `connlog-protocol-${protocol}`;
}

/** Fluent id of a level's name. */
export function levelId(level: WireLevel): `connlog-level-${WireLevel}` {
  return `connlog-level-${level}`;
}

/** Fluent id of a direction's name. */
export function directionId(direction: WireDirection): `connlog-direction-${WireDirection}` {
  return `connlog-direction-${direction}`;
}

/** Fluent id of what Nail was doing. */
export function stepId(step: ErrorStep): `error-step-${ErrorStep}` {
  return `error-step-${step}`;
}

/** A stable key of a scope (query keys, store maps). */
export function scopeKey(scope: ConnectionLogScope): string {
  switch (scope.kind) {
    case "account":
      return `account:${scope.account_id}`;
    case "pim-source":
      return `source:${scope.source_id}`;
    case "setup":
      return "setup";
  }
}

const ACCOUNT_INSTANCE = "account-";
const SOURCE_INSTANCE = "source-";
const SETUP_INSTANCE = "setup";

/**
 * The viewer window of a scope (`connection-log:account-work`); one
 * window per scope. `null` when the scope's id cannot name a window
 * instance (the viewer then opens without one and reads its scope from the
 * store).
 */
export function viewerWindowOf(scope: ConnectionLogScope): string | null {
  const instance =
    scope.kind === "account"
      ? `${ACCOUNT_INSTANCE}${scope.account_id}`
      : scope.kind === "pim-source"
        ? `${SOURCE_INSTANCE}${scope.source_id}`
        : SETUP_INSTANCE;
  return isWindowInstance(instance) ? windowId(CONNECTION_LOG_KIND, instance) : null;
}

/** The scope a viewer window id names, if its instance does. */
export function scopeOfViewerWindow(id: string): ConnectionLogScope | null {
  const parts = parseWindowId(id);
  if (parts?.kind !== CONNECTION_LOG_KIND || parts.instance === null) return null;
  const { instance } = parts;
  if (instance === SETUP_INSTANCE) return { kind: "setup" };
  if (instance.startsWith(ACCOUNT_INSTANCE)) {
    return { kind: "account", account_id: instance.slice(ACCOUNT_INSTANCE.length) };
  }
  if (instance.startsWith(SOURCE_INSTANCE)) {
    return { kind: "pim-source", source_id: instance.slice(SOURCE_INSTANCE.length) };
  }
  return null;
}

/**
 * What the viewer holds after fetching `page` (the entries after the last
 * one held): new entries appended, entries the core dropped (or cleared)
 * removed — positions `1..=dropped` are gone —, at most
 * the core's `capacity`.
 */
export function mergeLog(
  held: readonly ConnectionLogEntry[],
  page: ConnectionLog,
): ConnectionLogEntry[] {
  const last = held.at(-1)?.seq ?? 0;
  const merged = [...held, ...page.entries.filter((e) => e.seq > last)].filter(
    (e) => e.seq > page.dropped,
  );
  return merged.length > page.capacity ? merged.slice(merged.length - page.capacity) : merged;
}

/** The viewer's filters. */
export interface LogFilter {
  /** `null` = every protocol. */
  protocol: WireProtocol | null;
  /** The lowest level shown. */
  level: WireLevel;
}

export const ALL_ENTRIES: LogFilter = { protocol: null, level: "info" };

/** Entries passing `filter`, up to position `until` (paused) when given. */
export function filterEntries(
  entries: readonly ConnectionLogEntry[],
  filter: LogFilter,
  until: number | null = null,
): ConnectionLogEntry[] {
  const min = WIRE_LEVELS.indexOf(filter.level);
  return entries.filter(
    (e) =>
      (filter.protocol === null || e.protocol === filter.protocol) &&
      WIRE_LEVELS.indexOf(e.level) >= min &&
      (until === null || e.seq <= until),
  );
}

/** The protocols that occur in `entries`, in the contract's order. */
export function protocolsIn(entries: readonly ConnectionLogEntry[]): WireProtocol[] {
  const present = new Set(entries.map((e) => e.protocol));
  return WIRE_PROTOCOLS.filter((p) => present.has(p));
}

/**
 * The entry to show for an error's `log_seq`: the last entry at or before
 * it (the failure's own lines come just before it).
 */
export function entryAt(
  entries: readonly ConnectionLogEntry[],
  seq: number | null | undefined,
): number | null {
  if (seq === null || seq === undefined) return null;
  let found: number | null = null;
  for (const entry of entries) {
    if (entry.seq <= seq) found = entry.seq;
    else break;
  }
  return found;
}
