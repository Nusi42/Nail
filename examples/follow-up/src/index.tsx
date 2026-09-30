/**
 * "Follow up" — the design's example plug-in, adapted to the read-only
 * preview API: a button in the reader toolbar (and Shift+F in the message
 * list) opens a dialog with a due date and a note, the reminder is kept in
 * the plug-in's storage, and a notification confirms it. The "Follow-ups"
 * panel lists every reminder.
 *
 * Permissions (manifest.json): mail.read (the message's subject and
 * sender), ui.dialogs, storage, notifications.
 */
import { definePlugin, isPluginApiError, nail, type PluginCommandContext } from "@nail/plugin-api";

import {
  isoDate,
  isPast,
  isReminder,
  KEY_PREFIX,
  reminderFrom,
  reminderKey,
  sortReminders,
  suggestedDue,
  toJson,
  type Reminder,
} from "./reminders";
import { strings } from "./strings";
import { duePanel, followUpDialog, REMOVE, REMOVE_PAST, SAVE } from "./ui";

/** The panel's name (`plugin-panel "follow-up.due"` in layout.kdl). */
const PANEL = "due";

/** Every stored reminder, soonest first. */
async function loadReminders(): Promise<Reminder[]> {
  const keys = await nail.storage.keys(KEY_PREFIX);
  const values = await Promise.all(keys.map((key) => nail.storage.get(key)));
  const reminders: Reminder[] = [];
  for (const value of values) if (isReminder(value)) reminders.push(value);
  return sortReminders(reminders);
}

/** Redraws the panel if it is open (it may not be). */
async function refreshPanel(): Promise<void> {
  try {
    await nail.ui.updatePanel(PANEL, duePanel(await loadReminders(), isoDate(new Date())));
  } catch (error) {
    nail.log.debug("panel not updated", error);
  }
}

/** The "Follow up…" command. */
async function followUp(ctx: PluginCommandContext): Promise<void> {
  const messageId = ctx.selection.openMessageId ?? ctx.selection.messageIds[0];
  if (!messageId) {
    await nail.ui.notify({ title: strings.noSelection, level: "warning" });
    return;
  }

  let message;
  try {
    message = await nail.mail.get(messageId);
  } catch (error) {
    nail.log.warn("cannot read the message", error);
    if (isPluginApiError(error, "forbidden") || isPluginApiError(error, "not_found")) {
      await nail.ui.notify({ title: strings.notReadable, level: "error" });
      return;
    }
    throw error;
  }

  const key = reminderKey(message.id);
  const stored = await nail.storage.get(key);
  const existing = isReminder(stored) ? stored : null;
  const days = nail.settings.get("default-days");
  const suggested = suggestedDue(typeof days === "number" ? days : 1, new Date());

  const result = await nail.ui.showDialog(followUpDialog(message, suggested, existing));
  if (result.button === REMOVE) {
    await nail.storage.delete(key);
    await nail.ui.notify({ title: strings.removed, level: "info" });
    await refreshPanel();
    return;
  }
  if (result.button !== SAVE) return;

  const reminder = reminderFrom(message, result.values, new Date());
  if (!reminder) return;
  try {
    await nail.storage.set(key, toJson(reminder));
  } catch (error) {
    nail.log.error("reminder not saved", error);
    await nail.ui.notify({ title: strings.notSaved, level: "error" });
    return;
  }
  await nail.ui.notify({ title: strings.saved(reminder.due), level: "success" });
  await refreshPanel();
}

export default definePlugin({
  activate(ctx) {
    ctx.commands.register("follow-up", followUp);
    ctx.panels.register(PANEL, {
      render: async () => duePanel(await loadReminders(), isoDate(new Date())),
      onAction: async (action) => {
        if (action.componentId !== REMOVE_PAST) return;
        const today = isoDate(new Date());
        for (const reminder of await loadReminders()) {
          if (isPast(reminder, today)) await nail.storage.delete(reminderKey(reminder.messageId));
        }
        await refreshPanel();
      },
    });
  },
});
