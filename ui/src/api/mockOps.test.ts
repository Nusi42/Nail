/**
 * The mock core's queue summary (ADR 0230): `GetOpsSummary`, `OpsChanged`
 * per batch, bulk Retry and Dismiss of failed writes.
 */
import { TransportClient } from "./client";
import type { Event as NailEvent, FlagChange, OpsSummary } from "./generated";
import { MockTransport } from "./mock";
import { createMockData } from "./mockData";
import { opsSummaryOf, type MockOpQueue } from "./mockOps";

const FLAG: FlagChange = {
  seen: null,
  flagged: true,
  answered: null,
  draft: null,
  add_tags: [],
  remove_tags: [],
};

const EMPTY: OpsSummary = {
  pending: 0,
  in_flight: 0,
  failed: 0,
  accounts: [],
  newest_failure: null,
};

function setup() {
  const transport = new MockTransport({ data: createMockData(20) });
  const events: NailEvent[] = [];
  transport.onEvent((e) => events.push(e));
  const client = new TransportClient(transport);
  const summary = async () =>
    (await client.request({ type: "GetOpsSummary" }, "OpsSummary")).summary;
  const summaries = () => events.flatMap((e) => (e.type === "OpsChanged" ? [e.summary] : []));
  return { transport, client, summary, summaries };
}

describe("the mock core's queue summary", () => {
  it("sums accounts and keeps the newest failure", () => {
    const queue: MockOpQueue = new Map([
      ["op-1", { accountId: "a", state: "pending", error: null, failedAt: null }],
      ["op-2", { accountId: "a", state: "in-flight", error: null, failedAt: null }],
      [
        "op-3",
        { accountId: "b", state: "failed", error: "network", failedAt: "2026-11-21T10:00:00Z" },
      ],
      [
        "op-4",
        { accountId: "b", state: "failed", error: "not_found", failedAt: "2026-11-21T10:00:01Z" },
      ],
    ]);
    const summary = opsSummaryOf(queue);
    expect(summary).toMatchObject({ pending: 1, in_flight: 1, failed: 2 });
    expect(summary.accounts.map((a) => a.account_id)).toEqual(["a", "b"]);
    expect(summary.newest_failure).toMatchObject({ op_id: "op-4", code: "not_found" });
    expect(opsSummaryOf(new Map())).toEqual(EMPTY);
  });

  it("reports a batch once and ends empty", async () => {
    const { client, summary, summaries } = setup();
    expect(await summary()).toEqual(EMPTY);
    await client.request(
      {
        type: "UpdateFlags",
        message_ids: ["acc-personal/Inbox#2", "acc-personal/Inbox#3"],
        change: FLAG,
      },
      "Queued",
    );
    expect(summaries()[0]).toMatchObject({ pending: 1 });
    await vi.waitFor(async () => {
      expect(await summary()).toEqual(EMPTY);
    });
    expect(summaries().at(-1)).toEqual(EMPTY);
  });

  it("retries and dismisses failed writes in bulk", async () => {
    const { client, transport, summary } = setup();
    transport.data.behaviour.opError = "network";
    const { op_ids } = await client.request(
      { type: "UpdateFlags", message_ids: ["acc-personal/Inbox#2"], change: FLAG },
      "Queued",
    );
    await vi.waitFor(async () => {
      expect((await summary()).failed).toBe(1);
    });
    expect((await summary()).newest_failure).toMatchObject({ op_id: op_ids[0], code: "network" });
    expect(
      await client.request({ type: "RetryFailedOps", account_id: "nobody" }, "Queued"),
    ).toEqual({ type: "Queued", op_ids: [] });
    expect(await client.request({ type: "RetryFailedOps", account_id: null }, "Queued")).toEqual({
      type: "Queued",
      op_ids,
    });
    await vi.waitFor(async () => {
      expect((await summary()).failed).toBe(1);
    });
    expect(await client.call({ type: "DismissFailedOps", account_id: null })).toEqual({
      type: "Done",
    });
    expect(await summary()).toEqual(EMPTY);
  });
});
