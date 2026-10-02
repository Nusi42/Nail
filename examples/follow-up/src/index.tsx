/**
 * "Follow up" — the design's example plug-in: a button in the reader
 * toolbar (and Shift+F in the message list) opens a dialog with a due
 * date, a task list and a note, and creates a task linked to the message
 * (`nail.tasks.create`). The plug-in remembers the follow-up in its
 * storage: its "Follow-ups" panel lists them, and a background job
 * (hourly and when Nail starts) shows a notification when one is due.
 *
 * Permissions (manifest.json): mail.read (the message's subject and
 * sender), tasks.write (the task), ui.dialogs, notifications, storage,
 * background (the reminder job). Creating a task runs without asking
 * unless the user makes task writes ask in the plug-in's policies; then
 * `nail.tasks.create` waits for the user's decision.
 */
import {
  createdId,
  definePlugin,
  isPluginApiError,
  nail,
  type PluginCollection,
  type PluginCommandContext,
} from "@nail/plugin-api";

import {
  dueForReminder,
  followUpKey,
  inputFrom,
  isFollowUp,
  isoDate,
  isPast,
  KEY_PREFIX,
  senderOf,
  sortFollowUps,
  suggestedDue,
  toJson,
  type FollowUp,
} from "./reminders";
import { CREATE, duePanel, followUpDialog, REMOVE_PAST } from "./ui";

/** The panel's name (`plugin-panel "follow-up.due"` in layout.kdl). */
const PANEL = "due";

const t = (id: string, args?: Record<string, string | number>) => nail.l10n.t(id, args);

/** Every stored follow-up, soonest first. */
async function loadFollowUps(): Promise<FollowUp[]> {
  const keys = await nail.storage.keys(KEY_PREFIX);
  const values = await Promise.all(keys.map((key) => nail.storage.get(key)));
  const items: FollowUp[] = [];
  for (const value of values) if (isFollowUp(value)) items.push(value);
  return sortFollowUps(items);
}

/** Redraws the panel if it is open (it may not be). */
async function refreshPanel(): Promise<void> {
  try {
    await nail.ui.updatePanel(PANEL, duePanel(await loadFollowUps(), isoDate(new Date())));
  } catch (error) {
    nail.log.debug("panel not updated", error);
  }
}

/** The task lists, or `null` (after telling the user) when there are none or no access. */
async function taskLists(): Promise<PluginCollection[] | null> {
  try {
    const lists = (await nail.tasks.lists()).filter((l) => !l.readOnly);
    if (lists.length > 0) return lists;
    await nail.ui.notify({ title: t("no-lists"), level: "warning" });
  } catch (error) {
    if (!isPluginApiError(error, "forbidden")) throw error;
    await nail.ui.notify({ title: t("no-task-access"), level: "error" });
  }
  return null;
}

/** The "Follow up…" command. */
async function followUp(ctx: PluginCommandContext): Promise<void> {
  const messageId = ctx.selection.openMessageId ?? ctx.selection.messageIds[0];
  if (!messageId) {
    await nail.ui.notify({ title: t("no-selection"), level: "warning" });
    return;
  }

  let message;
  try {
    message = await nail.mail.get(messageId);
  } catch (error) {
    nail.log.warn("cannot read the message", error);
    if (isPluginApiError(error, "forbidden") || isPluginApiError(error, "not_found")) {
      await nail.ui.notify({ title: t("not-readable"), level: "error" });
      return;
    }
    throw error;
  }

  const lists = await taskLists();
  if (!lists) return;
  const key = followUpKey(message.id);
  const stored = await nail.storage.get(key);
  const existing = isFollowUp(stored) ? stored : null;
  const days = nail.settings.get("default-days");
  const suggested = suggestedDue(typeof days === "number" ? days : 1, new Date());
  const remind = nail.settings.get("remind") !== false;

  const answer = await nail.ui.showDialog(
    followUpDialog(message, suggested, lists, existing, remind),
  );
  if (answer.button !== CREATE) return;
  const input = inputFrom(answer.values);
  if (!input) return;

  const subject = message.subject || t("no-subject");
  const result = await nail.tasks.create({
    list: input.list ?? (lists.find((l) => l.default) ?? lists[0])?.id,
    title: t("task-title", { subject }),
    due: input.due,
    ...(input.note ? { notes: input.note } : {}),
    links: [message.uri],
  });
  if (result.status !== "done") {
    const reason = result.status === "expired" ? "not-created-expired" : "not-created-denied";
    await nail.ui.notify({ title: t(reason), level: "info" });
    return;
  }
  const taskId = createdId(result) ?? "";

  const item: FollowUp = {
    messageId: message.id,
    messageUri: message.uri,
    subject: message.subject,
    from: senderOf(message),
    taskId,
    due: input.due,
    note: input.note,
    remind: input.remind,
    createdAt: new Date().toISOString(),
  };
  try {
    await nail.storage.set(key, toJson(item));
  } catch (error) {
    nail.log.error("follow-up not saved", error);
    await nail.ui.notify({ title: t("not-saved"), level: "warning" });
    return;
  }
  await nail.ui.notify({ title: t("created", { due: input.due }), level: "success" });
  await refreshPanel();
}

/** The reminder job: one notification per follow-up that became due. */
async function remindDue(): Promise<void> {
  if (nail.settings.get("remind") === false) return;
  const today = isoDate(new Date());
  for (const item of dueForReminder(await loadFollowUps(), today)) {
    const title =
      item.due < today
        ? t("overdue", { due: item.due, subject: item.subject || t("no-subject") })
        : t("due-today", { subject: item.subject || t("no-subject") });
    await nail.ui.notify({ title, level: "info", system: true });
    await nail.storage.set(followUpKey(item.messageId), toJson({ ...item, notifiedOn: today }));
  }
}

export default definePlugin({
  commands: { "follow-up": followUp },
  panels: {
    [PANEL]: {
      render: async () => duePanel(await loadFollowUps(), isoDate(new Date())),
      onAction: async (action) => {
        if (action.componentId !== REMOVE_PAST) return;
        const today = isoDate(new Date());
        for (const item of await loadFollowUps()) {
          if (isPast(item, today)) await nail.storage.delete(followUpKey(item.messageId));
        }
        await refreshPanel();
      },
    },
  },
  jobs: { remind: remindDue },
});
