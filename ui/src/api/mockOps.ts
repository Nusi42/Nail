/**
 * The mock core's queue of message writes (ADR 0230): every queued write
 * with its state, the summary `GetOpsSummary` answers and `OpsChanged`
 * carries (one per queued batch and one per replayed write here; the core
 * coalesces further), Retry / Dismiss of failed writes. Each write still
 * reports `OpStatus` too until the UI no longer needs it (ADR 0230).
 */
import type {
  AccountId,
  ErrorCode,
  OpFailure,
  OpsAccountSummary,
  OpsSummary,
  OpState,
  Response,
} from "./generated";
import type { MockCore } from "./mockMail";
import { errorResponse } from "./mockMail";

/** One queued write of the mock core. */
export interface MockQueuedOp {
  accountId: AccountId;
  state: OpState;
  error: ErrorCode | null;
  /** When it failed (ISO 8601), for the newest failure. */
  failedAt: string | null;
}

/** The queue (op id → write), in enqueue order; `done` writes are removed. */
export type MockOpQueue = Map<string, MockQueuedOp>;

/** The summary of `queue`, as the core builds it. */
export function opsSummaryOf(queue: MockOpQueue): OpsSummary {
  const accounts = new Map<AccountId, OpsAccountSummary>();
  let newest: OpFailure | null = null;
  for (const [opId, op] of queue) {
    let row = accounts.get(op.accountId);
    if (row === undefined) {
      row = { account_id: op.accountId, pending: 0, in_flight: 0, failed: 0 };
      accounts.set(op.accountId, row);
    }
    if (op.state === "pending") row.pending++;
    else if (op.state === "in-flight") row.in_flight++;
    else if (op.state === "failed") {
      row.failed++;
      const failedAt = op.failedAt ?? "";
      if (newest === null || failedAt >= newest.failed_at) {
        newest = {
          op_id: opId,
          account_id: op.accountId,
          code: op.error ?? "internal",
          message: "mock replay failed",
          failed_at: failedAt,
        };
      }
    }
  }
  const rows = [...accounts.values()];
  const sum = (pick: (row: OpsAccountSummary) => number) =>
    rows.reduce((total, row) => total + pick(row), 0);
  return {
    pending: sum((r) => r.pending),
    in_flight: sum((r) => r.in_flight),
    failed: sum((r) => r.failed),
    accounts: rows,
    newest_failure: newest,
  };
}

/** Sends `OpsChanged` with the queue's summary now. */
export function emitOpsChanged(core: MockCore): void {
  core.emit({ type: "OpsChanged", summary: opsSummaryOf(core.data.opQueue) });
}

function setState(core: MockCore, opId: string, op: MockQueuedOp, state: OpState): void {
  op.state = state;
  if (state === "done") core.data.opQueue.delete(opId);
  else core.data.opQueue.set(opId, op);
}

/** Queues writes (one per entry) and replays them asynchronously. */
export function enqueueOps(core: MockCore, ops: readonly { opId: string; accountId: AccountId }[]) {
  for (const { opId, accountId } of ops) {
    core.data.opQueue.set(opId, { accountId, state: "pending", error: null, failedAt: null });
  }
  emitOpsChanged(core);
  for (const { opId } of ops) replayOp(core, opId);
}

/** Replays one queued write asynchronously with `OpStatus` and `OpsChanged`. */
export function replayOp(core: MockCore, opId: string): void {
  const op = core.data.opQueue.get(opId);
  if (op === undefined) return;
  const base = { op_id: opId, account_id: op.accountId, error: null, message: null };
  core.later(() => {
    setState(core, opId, op, "in-flight");
    core.emit({ type: "OpStatus", status: { ...base, state: "in-flight" } });
    core.later(() => {
      const error = core.data.behaviour.opError;
      if (error) {
        op.error = error;
        op.failedAt = new Date().toISOString();
        setState(core, opId, op, "failed");
      } else {
        setState(core, opId, op, "done");
      }
      core.emit({
        type: "OpStatus",
        status: error
          ? { ...base, state: "failed", error, message: "mock replay failed" }
          : { ...base, state: "done" },
      });
      emitOpsChanged(core);
    });
  });
}

function requeue(core: MockCore, opId: string, op: MockQueuedOp): void {
  op.error = null;
  op.failedAt = null;
  setState(core, opId, op, "pending");
}

/** `RetryOp`: a failed write is queued and replayed again. */
export function retryOp(core: MockCore, opId: string): Response {
  const op = core.data.opQueue.get(opId);
  if (op?.state !== "failed") {
    return errorResponse("conflict", `operation ${opId} has not failed`);
  }
  requeue(core, opId, op);
  queueMicrotask(() => {
    core.emit({
      type: "OpStatus",
      status: {
        op_id: opId,
        account_id: op.accountId,
        error: null,
        message: null,
        state: "pending",
      },
    });
    emitOpsChanged(core);
  });
  replayOp(core, opId);
  return { type: "Queued", op_ids: [opId] };
}

function failedOps(core: MockCore, accountId: AccountId | null): [string, MockQueuedOp][] {
  return [...core.data.opQueue].filter(
    ([, op]) => op.state === "failed" && (accountId === null || op.accountId === accountId),
  );
}

/** `RetryFailedOps`: every failed write (of one account) is replayed again. */
export function retryFailedOps(core: MockCore, accountId: AccountId | null): Response {
  const failed = failedOps(core, accountId);
  for (const [opId, op] of failed) requeue(core, opId, op);
  if (failed.length > 0)
    queueMicrotask(() => {
      emitOpsChanged(core);
    });
  for (const [opId] of failed) replayOp(core, opId);
  return { type: "Queued", op_ids: failed.map(([opId]) => opId) };
}

/** `DismissFailedOps`: the failed writes (of one account) are forgotten. */
export function dismissFailedOps(core: MockCore, accountId: AccountId | null): Response {
  const failed = failedOps(core, accountId);
  for (const [opId] of failed) core.data.opQueue.delete(opId);
  if (failed.length > 0)
    queueMicrotask(() => {
      emitOpsChanged(core);
    });
  return { type: "Done" };
}
