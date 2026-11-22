/**
 * The move window (R4, ADR 0230): Enter never moves to a partial match —
 * the arrow keys or the mouse highlight an option, Enter takes it (or a
 * folder whose name equals the typed text); better matches come first; the
 * folder the mail is in is left out; Drafts, Trash and Junk ask first with
 * the message count. And the drafts safety: a non-draft in a Drafts folder
 * is not continued in the composer, and `mail.move-out-of-drafts` moves it
 * to the Inbox.
 */
import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import type { Folder, FolderRole, MessageSummary } from "../../api/generated";
import { listFolders } from "../../api/mockMail";
import { mockFolderId } from "../../api/mockData";
import { listSelectionFor } from "../../actions/mailTargets";
import { builtinActions, createDispatcher } from "../../actions/registry";
import { confirmedMoveRole, setPendingMove, sourceFolderOf } from "./move";
import { isDraftMessage, isMisplacedInDrafts } from "./run";
import { actions, actionInfo, MOVE_OUT_OF_DRAFTS_ACTION } from "./index";
import { conversationActions } from "../conversation-reader/actions";
import {
  filterFolders,
  initialOption,
  matchRank,
  MATCH_RANK,
  rankFolders,
  stepOption,
} from "../../widgets/FolderPicker";
import { queryKeys } from "../../state/queries";
import { createEnv, renderNode, widget, type Env } from "../../test/harness";
import { actionContext, cacheList, requestsOf } from "../../test/env";
import { must } from "../../test/must";
import { INBOX, TRASH } from "../../test/mockIds";

const RECEIPTS = mockFolderId("acc-personal", "Inbox/Receipts");
const ARCHIVE = mockFolderId("acc-personal", "Archive");
/** The personal account's Drafts folder, found by its role. */
const DRAFTS = must(
  listFolders(createEnv().transport.data, "acc-personal")?.find((f) => f.role === "drafts"),
).id;

/** The move window of the personal account, opened for message `#2` of the Inbox (cached in its list). */
function openPicker(): { env: Env; summary: MessageSummary } {
  const message = `${INBOX}#2`;
  const env = createEnv({
    state: { activeAccountId: "acc-personal", selectedMessageId: message, windows: ["move"] },
  });
  const summary = must(env.transport.data.messages.find((m) => m.id === message));
  cacheList(env, [summary]);
  renderNode(widget("folder-picker"), env);
  return { env, summary };
}

const filterField = () => screen.getByRole("combobox", { name: "Find a folder" });
const option = (name: string) => screen.getByRole("option", { name });
/** The question before a move into Drafts, Trash or Junk (named by its text). */
const findQuestion = () => screen.findByRole("group", { name: /^Move \d+ messages? to / });
const moves = (env: Env) => requestsOf(env, "ApplyMailAction");

/** The move window's question after picking Trash. */
async function askTrash() {
  const user = userEvent.setup();
  const { env } = openPicker();
  await user.click(await screen.findByRole("option", { name: "Trash" }));
  return { user, env, question: await findQuestion() };
}

/** The window closed after moving the selected message (`#2` of the Inbox) to `folderId`. */
async function expectMoved(env: Env, folderId: string) {
  await waitFor(() => {
    expect(env.store.getState().windows).toEqual([]);
  });
  expect(moves(env)).toEqual([
    {
      type: "ApplyMailAction",
      targets: [{ kind: "message", message_id: `${INBOX}#2` }],
      action: { type: "move", target: { folder_id: folderId } },
    },
  ]);
}

describe("the move window", () => {
  it("moves the selected message to a clicked folder and closes", async () => {
    const user = userEvent.setup();
    const { env } = openPicker();
    await screen.findByRole("option", { name: "Archive" });
    await user.type(filterField(), "rec");
    expect(screen.queryByRole("option", { name: "Archive" })).toBeNull();
    await user.click(option("Receipts"));
    await expectMoved(env, RECEIPTS);
  });

  it("never moves to a partial match on Enter: arrows highlight, Enter takes the highlight", async () => {
    const user = userEvent.setup();
    const { env } = openPicker();
    await screen.findByRole("option", { name: "Archive" });
    const input = filterField();
    await user.type(input, "arch{Enter}");
    expect(await screen.findByRole("status")).toHaveTextContent(
      "Choose a folder with the arrow keys, then press Enter.",
    );
    expect(moves(env)).toEqual([]);
    expect(input).not.toHaveAttribute("aria-activedescendant");
    expect(env.store.getState().windows).toEqual(["move"]);

    await user.keyboard("{ArrowDown}");
    const archive = option("Archive");
    expect(archive).toHaveAttribute("aria-selected", "true");
    expect(archive).toHaveAttribute("data-state", "active");
    expect(input).toHaveAttribute("aria-activedescendant", archive.id);
    expect(screen.getByRole("status")).toBeEmptyDOMElement();
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(moves(env)).toHaveLength(1);
    });
    expect(moves(env)[0]?.action).toEqual({ type: "move", target: { folder_id: ARCHIVE } });
  });

  it("highlights a folder whose name equals the text, so Enter moves there", async () => {
    const user = userEvent.setup();
    const { env } = openPicker();
    await screen.findByRole("option", { name: "Receipts" });
    await user.type(filterField(), "RECEIPTS");
    expect(option("Receipts")).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(moves(env)[0]?.action).toEqual({ type: "move", target: { folder_id: RECEIPTS } });
    });
  });

  it("moves the highlight with the arrow keys and the mouse, within the list", async () => {
    const user = userEvent.setup();
    openPicker();
    await screen.findByRole("option", { name: "Archive" });
    const input = filterField();
    await user.click(input);
    await user.keyboard("{ArrowUp}");
    const all = screen.getAllByRole("option");
    expect(all.at(-1)).toHaveAttribute("aria-selected", "true");
    await user.keyboard("{ArrowDown}");
    expect(all.at(-1)).toHaveAttribute("aria-selected", "true");
    await user.hover(option("Sent"));
    expect(option("Sent")).toHaveAttribute("aria-selected", "true");
    expect(screen.getAllByRole("option", { selected: true })).toHaveLength(1);
    // Typing starts over: nothing highlighted for a partial match.
    await user.type(input, "s");
    expect(screen.queryAllByRole("option", { selected: true })).toEqual([]);
  });

  it("leaves out the folder the mail is already in", async () => {
    openPicker();
    await screen.findByRole("option", { name: "Archive" });
    expect(screen.queryByRole("option", { name: "Inbox" })).toBeNull();
    // Its subfolders stay.
    expect(option("Receipts")).toBeInTheDocument();
  });

  it("asks before moving into Trash, with the count; Cancel moves nothing", async () => {
    const { user, env, question } = await askTrash();
    expect(question).toHaveAttribute("data-role", "trash");
    expect(question).toHaveTextContent("Move 1 message to Trash?");
    expect(question).toHaveTextContent("Messages in Trash may be deleted for good later.");
    expect(within(question).getByRole("button", { name: "Cancel" })).toHaveFocus();
    expect(requestsOf(env, "ResolveMailTargets")).toEqual([
      {
        type: "ResolveMailTargets",
        targets: [{ kind: "message", message_id: `${INBOX}#2` }],
        action: { type: "move", target: { folder_id: TRASH } },
      },
    ]);
    await user.click(within(question).getByRole("button", { name: "Cancel" }));
    expect(moves(env)).toEqual([]);
    await waitFor(() => {
      expect(filterField()).toHaveFocus();
    });
    expect(env.store.getState().windows).toEqual(["move"]);
  });

  it("gives the focus back to Cancel when the core refuses the move", async () => {
    const { user, env, question } = await askTrash();
    // Gone meanwhile (another window): the core refuses the move.
    env.transport.data.messages = env.transport.data.messages.filter((m) => m.id !== `${INBOX}#2`);
    await user.click(within(question).getByRole("button", { name: "Move" }));
    await waitFor(() => {
      expect(within(question).getByRole("button", { name: "Cancel" })).toHaveFocus();
    });
    expect(question).toHaveAttribute("aria-busy", "false");
    expect(env.store.getState().windows).toEqual(["move"]);
  });

  it("moves exactly the counted messages into Drafts once confirmed", async () => {
    const user = userEvent.setup();
    const { env } = openPicker();
    await screen.findByRole("option", { name: "Drafts" });
    await user.type(filterField(), "drafts");
    await user.keyboard("{Enter}");
    const question = await findQuestion();
    expect(question).toHaveTextContent("Move 1 message to Drafts?");
    expect(question).toHaveTextContent("opens in the reader");
    await user.click(within(question).getByRole("button", { name: "Move" }));
    await expectMoved(env, DRAFTS);
  });

  it("asks with the conversation's message count for a conversation target", async () => {
    const user = userEvent.setup();
    const env = createEnv({
      state: { activeAccountId: "acc-personal", selectedMessageId: `${INBOX}#2`, windows: [] },
    });
    const thread = must(
      must(env.transport.data.messages.find((m) => m.id === `${INBOX}#2`)).thread_id,
    );
    setPendingMove(env.store, {
      targets: [{ kind: "conversation", thread_id: thread, folder_id: INBOX }],
      source: "list",
    });
    renderNode(widget("folder-picker"), env);
    await user.click(await screen.findByRole("option", { name: "Junk" }));
    const question = await findQuestion();
    const inThread = env.transport.data.messages.filter(
      (m) => m.thread_id === thread && m.folder_id === INBOX,
    );
    expect(inThread.length).toBeGreaterThan(1);
    expect(question).toHaveTextContent(`Move ${String(inThread.length)} messages to Junk?`);
    expect(question).toHaveTextContent("Messages in Junk are treated as spam.");
    await user.click(within(question).getByRole("button", { name: "Move" }));
    await waitFor(() => {
      expect(moves(env)).toHaveLength(1);
    });
    // The counted messages, not the conversation (nothing new moves unasked).
    const moved = must(moves(env)[0]).targets;
    expect(moved).toHaveLength(inThread.length);
    expect(moved).toEqual(
      expect.arrayContaining(inThread.map((m) => ({ kind: "message", message_id: m.id }))),
    );
  });

  it("asks once with the true count for a whole list and moves it with its mark", async () => {
    const user = userEvent.setup();
    // Select all keeps the row the list had selected.
    const env = createEnv({
      state: { activeAccountId: "acc-personal", selectedMessageId: `${INBOX}#2`, windows: [] },
    });
    setPendingMove(env.store, {
      targets: [
        { kind: "list", list: { source: "messages", folder_id: INBOX }, except: [], as_of: null },
      ],
      source: "list",
    });
    renderNode(widget("folder-picker"), env);
    await user.click(await screen.findByRole("option", { name: "Trash" }));
    const question = await findQuestion();
    // A list's answer has no ids: the count is the core's `messages`.
    const inInbox = env.transport.data.messages.filter((m) => m.folder_id === INBOX).length;
    expect(question).toHaveTextContent(`Move ${String(inInbox)} messages to Trash?`);
    expect(question).toHaveTextContent("Messages in Trash may be deleted for good later.");
    await user.click(within(question).getByRole("button", { name: "Move" }));
    await waitFor(() => {
      expect(moves(env)).toHaveLength(1);
    });
    // One request: the list with the mark, never a second bulk question.
    expect(must(moves(env)[0]).targets).toEqual([
      expect.objectContaining({ kind: "list", as_of: expect.stringMatching(/./) as string }),
    ]);
    expect(env.store.getState().windows).not.toContain("mail-bulk-confirm");
  });

  it("needs a selection", async () => {
    const user = userEvent.setup();
    const env = createEnv({ state: { activeAccountId: "acc-personal" } });
    renderNode(widget("folder-picker"), env);
    expect(await screen.findByText("Select a message first.")).toBeInTheDocument();
    const archive = await screen.findByRole("option", { name: "Archive" });
    expect(archive).toHaveAttribute("aria-disabled", "true");
    await user.click(archive);
    await user.type(filterField(), "archive{Enter}");
    expect(moves(env)).toEqual([]);
    expect(requestsOf(env, "ResolveMailTargets")).toEqual([]);
  });
});

describe("move window ranking", () => {
  const f = (name: string, path: string, role: FolderRole = "other") =>
    ({ id: path, name, path, role, parent_id: null }) as Folder;
  const row = (folder: Folder) => ({ folder, depth: 0 });

  it("ranks exact, prefix, contained name and path matches in that order", () => {
    const rows = [
      row(f("Old receipts", "Old receipts")),
      row(f("Bills", "Rec/Bills")),
      row(f("Receipts", "Inbox/Receipts")),
      row(f("Rec", "Rec")),
      row(f("Sent", "Sent")),
    ];
    expect(rankFolders(rows, " rec ").map((r) => r.folder.path)).toEqual([
      "Rec",
      "Inbox/Receipts",
      "Old receipts",
      "Rec/Bills",
    ]);
    expect(rankFolders(rows, "")).toEqual(rows);
    expect(matchRank(f("Rec", "Rec"), "rec")).toBe(MATCH_RANK.exact);
    expect(matchRank(f("Sent", "Sent"), "rec")).toBeNull();
  });

  it("highlights only a single exact name match", () => {
    const one = rankFolders(
      [row(f("Archive", "Archive")), row(f("Archives", "Archives"))],
      "archive",
    );
    expect(initialOption(one, "archive")).toBe(0);
    const two = [row(f("Archive", "A/Archive")), row(f("Archive", "B/Archive"))];
    expect(initialOption(two, "archive")).toBeNull();
    expect(initialOption(one, "arch")).toBeNull();
    expect(initialOption(one, " ")).toBeNull();
  });

  it("steps the highlight within the list", () => {
    expect(stepOption(null, 3, 1)).toBe(0);
    expect(stepOption(null, 3, -1)).toBe(2);
    expect(stepOption(2, 3, 1)).toBe(2);
    expect(stepOption(0, 3, -1)).toBe(0);
    expect(stepOption(1, 3, 1)).toBe(2);
    expect(stepOption(null, 0, 1)).toBeNull();
  });

  it("filters by name and path", () => {
    expect(filterFolders([f("Bills", "Inbox/Bills"), f("Sent", "Sent")], "inbox")).toHaveLength(1);
    expect(filterFolders([f("Bills", "Inbox/Bills")], "  ")).toHaveLength(1);
  });

  it("asks before Drafts, Trash and Junk only", () => {
    const roles: FolderRole[] = ["inbox", "sent", "drafts", "archive", "junk", "trash", "other"];
    expect(roles.filter((role) => confirmedMoveRole({ role }) !== null)).toEqual([
      "drafts",
      "junk",
      "trash",
    ]);
  });

  it("knows the folder the targets are in", () => {
    const env = createEnv();
    const a = must(env.transport.data.messages.find((m) => m.id === `${INBOX}#1`));
    cacheList(env, [a]);
    const conversation = { kind: "conversation", thread_id: "t", folder_id: ARCHIVE } as const;
    expect(sourceFolderOf(env, [{ kind: "message", message_id: a.id }])).toBe(INBOX);
    expect(sourceFolderOf(env, [conversation])).toBe(ARCHIVE);
    expect(sourceFolderOf(env, [conversation, { kind: "message", message_id: a.id }])).toBeNull();
    expect(sourceFolderOf(env, [{ kind: "message", message_id: "unknown" }])).toBeNull();
    expect(sourceFolderOf(env, [])).toBeNull();
  });
});

/** Runs Move out of Drafts: exactly message `id` moves to the Inbox. */
async function expectMovedOutOfDrafts(
  env: Env,
  dispatcher: ReturnType<typeof createDispatcher>,
  id: string,
) {
  await dispatcher.dispatch(MOVE_OUT_OF_DRAFTS_ACTION);
  expect(moves(env)).toEqual([
    {
      type: "ApplyMailAction",
      targets: [{ kind: "message", message_id: id }],
      action: { type: "move", target: { role: "inbox" } },
    },
  ]);
}

describe("drafts safety", () => {
  /** A received message (no `\Draft` flag) moved into the personal Drafts folder, listed there. */
  function receivedInDrafts(): { env: Env; message: MessageSummary } {
    const env = createEnv({ state: { activeAccountId: "acc-personal", activeFolderId: DRAFTS } });
    const message = must(env.transport.data.messages.find((m) => m.id === `${INBOX}#1`));
    message.folder_id = DRAFTS;
    cacheList(env, [message]);
    env.queryClient.setQueryData(
      queryKeys.folders("acc-personal"),
      listFolders(env.transport.data, "acc-personal"),
    );
    env.store.getState().selectMessage(message.id);
    return { env, message };
  }

  it("opens a non-draft of a Drafts folder in the reader, not the composer", async () => {
    const { env, message } = receivedInDrafts();
    const ctx = actionContext(env);
    expect(isDraftMessage(ctx, message.id)).toBe(false);
    const dispatcher = createDispatcher(ctx, { ...builtinActions, ...conversationActions });
    expect(dispatcher.isEnabled("mail.continue-draft")).toBe(false);
    await dispatcher.dispatch("conversation.open-in-reader");
    await dispatcher.dispatch("mail.continue-draft");
    expect(requestsOf(env, "EditDraftMessage")).toEqual([]);
  });

  it("offers to move it out of Drafts, into the Inbox", async () => {
    const { env, message } = receivedInDrafts();
    const ctx = actionContext(env);
    expect(isMisplacedInDrafts(ctx, message.id)).toBe(true);
    const dispatcher = createDispatcher(ctx, { ...builtinActions, ...actions });
    expect(dispatcher.isEnabled(MOVE_OUT_OF_DRAFTS_ACTION)).toBe(true);
    expect(actionInfo[MOVE_OUT_OF_DRAFTS_ACTION]?.title).toBe(
      "mail-actions-action-move-out-of-drafts",
    );
    await expectMovedOutOfDrafts(env, dispatcher, message.id);
    expect(env.transport.data.messages.find((m) => m.id === message.id)?.folder_id).toBe(INBOX);
  });

  it("moves only the selected message, never picked rows or its conversation's drafts", async () => {
    const { env, message } = receivedInDrafts();
    const draft = must(env.transport.data.messages.find((m) => m.id === `${INBOX}#3`));
    draft.folder_id = DRAFTS;
    draft.flags = { ...draft.flags, draft: true };
    draft.thread_id = message.thread_id;
    cacheList(env, [message, draft]);
    // Both rows picked, in a list of conversations: `currentTargets` would
    // name the real draft too.
    listSelectionFor(env.store).setState({
      scope: { conversations: true, folderId: DRAFTS },
      picked: [message, draft].map((m) => ({
        key: m.id,
        messageId: m.id,
        threadId: m.thread_id,
        child: false,
      })),
    });
    const ctx = actionContext(env);
    const dispatcher = createDispatcher(ctx, { ...builtinActions, ...actions });
    expect(dispatcher.isEnabled(MOVE_OUT_OF_DRAFTS_ACTION)).toBe(true);
    await expectMovedOutOfDrafts(env, dispatcher, message.id);
    expect(env.transport.data.messages.find((m) => m.id === draft.id)?.folder_id).toBe(DRAFTS);
  });

  it("does not offer it for real drafts or mail outside Drafts", () => {
    const { env, message } = receivedInDrafts();
    const ctx = actionContext(env);
    message.flags = { ...message.flags, draft: true };
    cacheList(env, [message]);
    expect(isMisplacedInDrafts(ctx, message.id)).toBe(false);
    expect(isDraftMessage(ctx, message.id)).toBe(true);
    const inbox = must(env.transport.data.messages.find((m) => m.id === `${INBOX}#2`));
    cacheList(env, [inbox]);
    expect(isMisplacedInDrafts(ctx, inbox.id)).toBe(false);
    expect(isMisplacedInDrafts(ctx, null)).toBe(false);
    expect(isMisplacedInDrafts(ctx, "unknown")).toBe(false);
  });
});
