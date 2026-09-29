import { describe, expect, it } from "vitest";
import type { Event } from "../../api/generated";
import { MockTransport } from "../../api/mock";
import { SYNC_REQUESTS, isSyncRequest } from "./mock";

describe("sync mock", () => {
  it("routes exactly its requests", () => {
    expect(new Set(SYNC_REQUESTS).size).toBe(SYNC_REQUESTS.length);
    expect(isSyncRequest({ type: "FetchMessageBodies", message_ids: [] })).toBe(true);
    expect(isSyncRequest({ type: "Ping" })).toBe(false);
  });

  it("resolves and edits a folder's policy in the accounts configuration", async () => {
    const t = new MockTransport();
    const events: Event[] = [];
    t.onEvent((e) => events.push(e));
    const inbox = t.data.folders.find((f) => f.role === "inbox");
    if (!inbox) throw new Error("no inbox");
    expect(await t.send({ type: "GetFolderSyncPolicy", folder_id: inbox.id })).toMatchObject({
      type: "FolderSyncPolicy",
      policy: { bodies: { mode: "full" }, idle: true, rule: null, on_demand: true },
    });
    const set = await t.send({
      type: "SetFolderSyncPolicy",
      folder_id: inbox.id,
      bodies: { mode: "headers-only" },
      enabled: null,
      idle: false,
    });
    expect(set).toMatchObject({
      type: "FolderSyncPolicy",
      policy: {
        bodies: { mode: "headers-only" },
        idle: false,
        rule: { folder: { path: inbox.path } },
      },
    });
    expect(events).toContainEqual({ type: "AccountsChanged" });
    // All `null` removes the rule again.
    const cleared = await t.send({
      type: "SetFolderSyncPolicy",
      folder_id: inbox.id,
      bodies: null,
      enabled: null,
      idle: null,
    });
    expect(cleared).toMatchObject({ policy: { rule: null, bodies: { mode: "full" } } });
    expect(
      await t.send({
        type: "SetFolderSyncPolicy",
        folder_id: inbox.id,
        bodies: { mode: "recent", days: 0 },
        enabled: null,
        idle: null,
      }),
    ).toMatchObject({ code: "invalid_request" });

    const message = t.data.messages[0]?.id ?? "";
    expect(await t.send({ type: "FetchMessageBodies", message_ids: [message, "gone"] })).toEqual({
      type: "BodiesFetched",
      fetched: [message],
      failed: ["gone"],
    });
  });
});
