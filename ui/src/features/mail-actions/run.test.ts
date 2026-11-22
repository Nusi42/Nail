/**
 * `runMailAction` sends one request per action, whatever its size (R5,
 * ADR 0235): a confirmation's counted messages go as they are, and a whole
 * list ("select all") asks once with the true count, then goes as one list
 * target with the confirmation's `as_of` mark.
 */
import { act } from "@testing-library/react";
import type { MailAction, MessageTarget, Request } from "../../api/generated";
import { MAIL_ACTION_KIND_VALUES } from "../../api/generated/constants";
import { createEnv, type Env } from "../../test/harness";
import { eventually } from "../../test/app";
import { must } from "../../test/must";
import { INBOX, TRASH } from "../../test/mockIds";
import { createLocalizer } from "../../i18n/localizer";
import { shippedCatalogs } from "../../i18n/catalogs";
import { answerBulk, BULK_CONFIRM_DIALOG, bulkLabelArgs, pendingBulk } from "./bulk";
import { confirmedTargets, runMailAction, runOptionsOf } from "./run";

const applies = (env: Env) =>
  env.transport.requests.flatMap((r: Request) => (r.type === "ApplyMailAction" ? [r.targets] : []));

const inboxIds = (env: Env) =>
  env.transport.data.messages.filter((m) => m.folder_id === INBOX).map((m) => m.id);

/** The whole Inbox (a list without conversations) except `left`. */
const wholeInbox = (left: readonly string[] = []): MessageTarget => ({
  kind: "list",
  list: { source: "messages", folder_id: INBOX },
  except: left.map((id) => ({ kind: "message", message_id: id })),
  as_of: null,
});

describe("runMailAction: one request per action", () => {
  it("applies a confirmation's counted messages in one request with one undo", async () => {
    const env = createEnv({ inboxSize: 600 });
    const ids = inboxIds(env);
    const resolution = {
      message_ids: ids,
      messages: ids.length,
      account_ids: [],
      conversations: 0,
      as_of: null,
    };
    const result = await act(() =>
      runMailAction(
        env,
        { targets: [{ kind: "conversation", thread_id: "t", folder_id: INBOX }], source: "list" },
        { type: "archive" },
        runOptionsOf(resolution),
      ),
    );
    const sent = applies(env);
    expect(sent).toHaveLength(1);
    expect(sent[0]).toEqual(ids.map((id) => ({ kind: "message", message_id: id })));
    expect(result?.message_ids).toHaveLength(ids.length);
    expect(result?.batch_id).not.toBeNull();
    expect(inboxIds(env)).toEqual([]);
  });

  it("asks once with the true count before acting on a whole list, then sends it with the mark", async () => {
    const env = createEnv({ inboxSize: 250 });
    const ids = inboxIds(env);
    const left = must(ids[0]);
    const running = runMailAction(
      env,
      { targets: [wholeInbox([left])], source: "list" },
      { type: "archive" },
    );
    await eventually(() => {
      expect(pendingBulk(env.store)).not.toBeNull();
    });
    const question = must(pendingBulk(env.store));
    expect(question.resolution.messages).toBe(ids.length - 1);
    expect(question.resolution.message_ids).toEqual([]);
    expect(env.store.getState().windows).toContain(BULK_CONFIRM_DIALOG);
    // Nothing is applied while the question is open.
    expect(applies(env)).toEqual([]);

    // Mail arriving while the question is open is not part of the action.
    const arrived = { ...must(env.transport.data.messages.find((m) => m.id === left)) };
    arrived.id = `${INBOX}#arrived`;
    env.transport.data.messages.push(arrived);

    answerBulk(env.store, true);
    const result = await act(() => running);
    const sent = applies(env);
    expect(sent).toHaveLength(1);
    expect(sent[0]).toEqual([{ ...wholeInbox([left]), as_of: question.resolution.as_of }]);
    expect(question.resolution.as_of).not.toBeNull();
    expect(result?.message_ids).toHaveLength(ids.length - 1);
    expect(result?.batch_id).not.toBeNull();
    expect(inboxIds(env).sort()).toEqual([left, arrived.id].sort());
    expect(env.store.getState().windows).not.toContain(BULK_CONFIRM_DIALOG);
  });

  it("acts on nothing when the question is cancelled or its window closes", async () => {
    const env = createEnv({ inboxSize: 10 });
    const cancelled = runMailAction(
      env,
      { targets: [wholeInbox()], source: "list" },
      { type: "mark-read", read: true },
    );
    await eventually(() => {
      expect(pendingBulk(env.store)).not.toBeNull();
    });
    answerBulk(env.store, false);
    expect(await act(() => cancelled)).toBeNull();

    const closed = runMailAction(
      env,
      { targets: [wholeInbox()], source: "list" },
      { type: "trash" },
    );
    await eventually(() => {
      expect(env.store.getState().windows).toContain(BULK_CONFIRM_DIALOG);
    });
    // Esc or the window's close button.
    act(() => {
      env.store.getState().closeWindow(BULK_CONFIRM_DIALOG);
    });
    expect(await act(() => closed)).toBeNull();
    expect(pendingBulk(env.store)).toBeNull();
    expect(applies(env)).toEqual([]);
    expect(inboxIds(env)).toHaveLength(10);
  });

  it("does not ask about a list that leaves nothing", async () => {
    const env = createEnv({ inboxSize: 3 });
    const result = await act(() =>
      runMailAction(
        env,
        { targets: [wholeInbox(inboxIds(env))], source: "list" },
        { type: "archive" },
      ),
    );
    expect(result).toBeNull();
    expect(pendingBulk(env.store)).toBeNull();
    expect(applies(env)).toEqual([]);
  });
});

describe("confirmedTargets", () => {
  const resolution = {
    message_ids: ["m1", "m2"],
    messages: 2,
    account_ids: [],
    conversations: 1,
    as_of: "mark-7",
  };

  it("sends a whole list with the confirmation's mark", () => {
    expect(confirmedTargets([wholeInbox(["x"])], resolution)).toEqual([
      { ...wholeInbox(["x"]), as_of: "mark-7" },
    ]);
  });

  it("sends exactly the counted messages otherwise", () => {
    expect(
      confirmedTargets([{ kind: "conversation", thread_id: "t", folder_id: TRASH }], resolution),
    ).toEqual([
      { kind: "message", message_id: "m1" },
      { kind: "message", message_id: "m2" },
    ]);
  });
});

describe("the question's button", () => {
  it("names every kind of action, each in its own words", () => {
    const actions: MailAction[] = [
      { type: "archive" },
      { type: "trash" },
      { type: "delete-permanently" },
      { type: "move", target: { role: "inbox" } },
      { type: "mark-read", read: true },
      { type: "mark-read", read: false },
      { type: "flag", flagged: true },
      { type: "flag", flagged: false },
      { type: "tags", add: ["a"], remove: [] },
      { type: "snooze", until: "2030-01-01T00:00:00Z", wake_folder_id: null },
      { type: "junk", junk: true },
      { type: "junk", junk: false },
    ];
    const { t } = createLocalizer(shippedCatalogs, ["en"]);
    const labels = actions.map((action) => t("mail-bulk-confirm", bulkLabelArgs(action)));
    // One label per kind of action the core knows.
    expect(new Set(labels).size).toBe(MAIL_ACTION_KIND_VALUES.length);
    expect(labels).not.toContain(t("mail-bulk-confirm", { action: "other", on: "yes" }));
    expect(labels[0]).toBe("Archive all");
    expect(labels[5]).toBe("Mark all as unread");
  });
});
