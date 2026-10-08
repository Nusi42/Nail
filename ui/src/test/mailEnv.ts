/**
 * The mail window's start state the window sweeps share (a11y, themes,
 * windows): Ada's personal Inbox with its first message selected and the
 * composer on her identity.
 */
import { emptyComposer } from "../state/compose";
import { createEnv, type Env } from "./harness";
import { INBOX } from "./mockIds";

/** A fresh env on the personal Inbox with its first message selected. */
export function mailEnv(): Env {
  const env = createEnv({
    state: {
      activeAccountId: "acc-personal",
      activeFolderId: INBOX,
      composer: { ...emptyComposer(), identityId: "ada@example.org" },
    },
  });
  const first = env.transport.data.messages.find((m) => m.folder_id === INBOX);
  if (first) env.store.getState().selectMessage(first.id);
  return env;
}
