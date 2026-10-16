/**
 * A queued write and its outcome (ADR 0196): the core answers `Queued`
 * with op ids and reports each op's progress as `OpStatus`. `runQueued`
 * sends the request and waits until every op is done (`"done"`), failed
 * (throws a `NailError` with the core's localized text) or waits for the
 * server (`"waiting"`: offline, retried later). Events are collected from
 * before the request, so a fast replay is not missed.
 */
import { NailError, type NailClient } from "../api/client";
import type { OpId, OpStatus, Request } from "../api/generated";

/** How long a dialog waits for the server before calling it "waiting". */
export const QUEUED_OP_WAIT_MS = 15_000;

export type QueuedOutcome = "done" | "waiting";

/** The outcome of `statuses` for `ops`, `null` while undecided. */
export function outcomeOf(
  ops: readonly OpId[],
  statuses: ReadonlyMap<OpId, OpStatus>,
): QueuedOutcome | OpStatus | null {
  const latest = ops.map((op) => statuses.get(op));
  const failed = latest.find((s) => s?.state === "failed");
  if (failed) return failed;
  if (latest.every((s) => s?.state === "done")) return "done";
  // A retry waits: the op is pending again with an error.
  if (latest.some((s) => s?.state === "pending" && s.error !== null)) return "waiting";
  return null;
}

/** Sends `request` (answered `Queued`) and waits for its ops. */
export async function runQueued(client: NailClient, request: Request): Promise<QueuedOutcome> {
  const statuses = new Map<OpId, OpStatus>();
  let changed: () => void = () => undefined;
  const off = client.subscribe((event) => {
    if (event.type === "OpStatus") {
      statuses.set(event.status.op_id, event.status);
      changed();
    }
  });
  try {
    const { op_ids } = await client.request(request, "Queued");
    return await new Promise<QueuedOutcome>((resolve, reject) => {
      const timer = setTimeout(() => {
        resolve("waiting");
      }, QUEUED_OP_WAIT_MS);
      changed = () => {
        const outcome = outcomeOf(op_ids, statuses);
        if (outcome === null) return;
        clearTimeout(timer);
        if (typeof outcome === "string") {
          resolve(outcome);
        } else {
          reject(
            new NailError(
              outcome.error ?? "internal",
              outcome.message ?? "",
              outcome.localized ?? null,
            ),
          );
        }
      };
      changed();
    });
  } finally {
    off();
  }
}
