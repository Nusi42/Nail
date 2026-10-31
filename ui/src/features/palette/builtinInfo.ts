/**
 * Command-palette descriptions of the actions that do not describe
 * themselves (`Feature.actionInfo`): the built-ins and the actions of the
 * features built before M4c. Titles are used when the layout gives the
 * action no label; keywords make it findable by other words; availability
 * says where it applies (ADR 0094). Actions that only make sense with
 * arguments (`window.open "<name>"`, `mail.move-to "<folder>"`) are
 * hidden: the layout's aliases for them are listed instead.
 *
 * A feature's own `actionInfo` wins over an entry here.
 */
import { MAIL_AVAILABILITY as MAIL, type ActionInfo } from "../../actions/palette";
import type { ActionAvailability } from "../../api/generated";

type Row = readonly [id: string, availability: readonly ActionAvailability[], keywords: boolean];

const LIST: readonly ActionAvailability[] = ["list"];
const READER: readonly ActionAvailability[] = ["reader"];
const COMPOSER: readonly ActionAvailability[] = ["composer"];
const CALENDAR: readonly ActionAvailability[] = ["calendar"];
const CONTACTS: readonly ActionAvailability[] = ["contacts"];
const DESKTOP: readonly ActionAvailability[] = ["desktop"];
const ANY: readonly ActionAvailability[] = ["global"];

/** `[action id, availability, has keywords]`. */
const ROWS: readonly Row[] = [
  ["account.add", ANY, true],
  ["agents.open-center", ANY, true],
  ["calendar.calendars", ANY, false],
  ["calendar.event-from-message", MAIL, true],
  ["calendar.goto-today", CALENDAR, false],
  ["calendar.new-event", ANY, true],
  ["calendar.next", CALENDAR, false],
  ["calendar.previous", CALENDAR, false],
  ["composer.attach", COMPOSER, true],
  ["composer.discard", COMPOSER, false],
  ["composer.save", COMPOSER, true],
  ["composer.send", COMPOSER, false],
  ["composer.toggle-preview", COMPOSER, false],
  ["composer.toggle-source", COMPOSER, false],
  ["contacts.groups", ANY, false],
  ["contacts.merge", CONTACTS, true],
  ["contacts.new", ANY, true],
  ["customize.errors", ANY, false],
  ["customize.merge", ANY, true],
  ["exchange.add-mailbox", ANY, true],
  ["exchange.apply-categories", MAIL, true],
  ["exchange.automatic-replies", ANY, true],
  ["exchange.classify", MAIL, false],
  ["exchange.new-category", ANY, false],
  ["exchange.toggle-focused", LIST, false],
  ["gmail.apply-labels", MAIL, true],
  ["gmail.category", LIST, false],
  ["gmail.new-label", ANY, false],
  ["inspector.toggle", ANY, true],
  ["junk.mark", MAIL, true],
  ["junk.not-junk", MAIL, false],
  ["mail.archive", MAIL, false],
  ["mail.compose", ANY, true],
  ["mail.delete", MAIL, true],
  ["mail.delete-permanently", MAIL, false],
  ["mail.forward", MAIL, false],
  ["mail.move", MAIL, true],
  ["mail.next", LIST, false],
  ["mail.previous", LIST, false],
  ["mail.reply", MAIL, true],
  ["mail.reply-all", MAIL, false],
  ["mail.toggle-flag", MAIL, true],
  ["mail.toggle-read", MAIL, true],
  ["outbox.undo", ANY, false],
  ["pim.import", ANY, true],
  ["pim.sources", ANY, true],
  ["pim.sync", ANY, false],
  ["plugins.install", ANY, false],
  ["plugins.load-dev", DESKTOP, true],
  ["plugins.manage", ANY, true],
  ["remote.content.allow", READER, true],
  ["remote.logout", ANY, false],
  ["remote.pair", ANY, true],
  ["rules.create-from-message", MAIL, true],
  ["rules.open", ANY, true],
  ["scheduling.accept", READER, false],
  ["scheduling.decline", READER, false],
  ["scheduling.find-time", ANY, true],
  ["scheduling.settings", ANY, false],
  ["scheduling.tentative", READER, false],
  ["search.clear", ANY, false],
  ["search.focus", ANY, true],
  ["security.autocrypt.import", READER, false],
  ["snooze.list", ANY, false],
  ["snooze.open", MAIL, true],
  ["sync.download-message", READER, false],
  ["sync.folder-policy", LIST, true],
  ["sync.now", ANY, true],
  ["system.about", ANY, true],
  ["system.check-updates", DESKTOP, true],
  ["system.default-apps", DESKTOP, true],
  ["system.first-run", DESKTOP, false],
  ["system.reload", ANY, true],
  ["tasks.from-message", MAIL, false],
  ["tasks.new", ANY, true],
  ["window.close", ANY, false],
];

/** Actions that need arguments or a pointer position: not listed. */
export const HIDDEN_ACTIONS: readonly string[] = [
  "calendar.view",
  "composer.set-mode",
  "exchange.focus",
  "gmail.label-view",
  "mail.move-to",
  "mcp.indicator.dismiss",
  "menu.context",
  "plugins.run-command",
  "view.toggle",
  "window.open",
];

/** Fluent id of a built-in action's palette title (`palette-action-mail-reply`). */
export function titleId(actionId: string): string {
  return `palette-action-${actionId.replaceAll(".", "-")}`;
}

/** Fluent id of its keywords. */
export function keywordsId(actionId: string): string {
  return `${titleId(actionId)}-keywords`;
}

/** The descriptions by action id. */
export const BUILTIN_INFO: Readonly<Record<string, ActionInfo>> = Object.fromEntries([
  ...ROWS.map(([id, availability, keywords]): [string, ActionInfo] => [
    id,
    {
      title: titleId(id),
      ...(keywords ? { keywords: keywordsId(id) } : {}),
      ...(availability === ANY ? {} : { availability }),
    },
  ]),
  ...HIDDEN_ACTIONS.map((id): [string, ActionInfo] => [id, { title: titleId(id), hidden: true }]),
]);

/** Every Fluent id this module names (catalog coverage). */
export function builtinMessageIds(): string[] {
  return ROWS.flatMap(([id, , keywords]) =>
    keywords ? [titleId(id), keywordsId(id)] : [titleId(id)],
  );
}
