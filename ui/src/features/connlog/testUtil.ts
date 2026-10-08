/**
 * Test helpers of the connection log feature (ADR 0190): a failed sync of
 * the mock account Work with its stored error.
 */
import { waitFor } from "@testing-library/react";
import { expect } from "vitest";
import type { Env } from "../../test/harness";
import { failSyncWith } from "./mock";

/**
 * Makes the mock core refuse Work's folder `Archive`, syncs Work and waits
 * until the error is stored.
 */
export async function failWorkSync(env: Env): Promise<void> {
  failSyncWith(env.transport, "acc-work", "Archive");
  await env.client.request({ type: "SyncNow", account_id: "acc-work" }, "SyncStarted");
  await waitFor(() => {
    expect(env.transport.data.features.connlog.errors).toHaveLength(1);
  });
}
