/**
 * `runMailAction` with explicit messages (P3 review): a confirmation's
 * counted messages are applied as they are, in requests of at most
 * `MAX_MAIL_ACTION_TARGETS` targets.
 */
import { act } from "@testing-library/react";
import type { MessageTarget } from "../../api/generated";
import { createEnv } from "../../test/harness";
import { MAX_MAIL_ACTION_TARGETS } from "../../api/generated/constants";
import { runMailAction } from "./run";

const TRASH = "acc-personal/Trash";

describe("runMailAction with explicit messages", () => {
  it("splits more than the request limit into several requests", async () => {
    const env = createEnv({ inboxSize: MAX_MAIL_ACTION_TARGETS + 20 });
    const inbox = env.transport.data.messages.filter((m) => m.folder_id === "acc-personal/Inbox");
    expect(inbox.length).toBeGreaterThan(MAX_MAIL_ACTION_TARGETS);
    for (const m of inbox) m.folder_id = TRASH;
    const apply: MessageTarget[] = inbox.map((m) => ({ kind: "message", message_id: m.id }));
    const result = await act(() =>
      runMailAction(
        env,
        { targets: [{ kind: "conversation", thread_id: "t", folder_id: TRASH }], source: "list" },
        { type: "delete-permanently" },
        { apply, conversations: 1 },
      ),
    );
    const sent = env.transport.requests.flatMap((r) =>
      r.type === "ApplyMailAction" ? [r.targets] : [],
    );
    expect(sent).toHaveLength(2);
    expect(sent.flat()).toEqual(apply);
    expect(result?.message_ids).toHaveLength(inbox.length);
    expect(result?.conversations).toBe(1);
    expect(result?.batch_id).toBeNull();
    expect(env.transport.data.messages.some((m) => m.folder_id === TRASH)).toBe(false);
  });
});
