/**
 * The reader action bar of the approved UX proposal (M5b WP162): Reply,
 * Reply all and Forward labelled (Reply is the primary one, `rank=1`);
 * Archive, Delete, Move, Flag and Snooze as icon buttons; everything else in
 * "More" under the headings Message and Turn into. The widget tests render
 * it, and it is the toolbar WP165 writes into `defaults/layout.kdl` (as
 * `toolbar "reader" hide-disabled=#true { … }`).
 */
export const PROPOSAL_READER = [
  { type: "action", action: "mail.reply", rank: 1 },
  { type: "action", action: "mail.reply-all", rank: 2 },
  { type: "action", action: "mail.forward", rank: 3 },
  { type: "separator" },
  { type: "action", action: "mail.archive", rank: 4, display: "icon" },
  { type: "action", action: "mail.delete", rank: 5, display: "icon" },
  { type: "action", action: "mail.move", rank: 6, display: "icon" },
  { type: "action", action: "mail.toggle-flag", rank: 7, display: "icon" },
  { type: "action", action: "snooze.open", rank: 8, display: "icon" },
  { type: "spacer" },
  {
    type: "more",
    items: [
      { type: "header", label: "@toolbar.group.message" },
      { type: "action", action: "mail.toggle-read" },
      { type: "action", action: "junk.mark" },
      // Only for Gmail / Exchange messages and Autocrypt setup messages:
      // hidden for the others (`hide-disabled`).
      { type: "action", action: "gmail.apply-labels" },
      { type: "action", action: "exchange.apply-categories" },
      { type: "action", action: "exchange.classify" },
      { type: "action", action: "security.autocrypt.import" },
      { type: "action", action: "print.message" },
      { type: "separator" },
      { type: "header", label: "@toolbar.group.turn-into" },
      { type: "action", action: "calendar.event-from-message" },
      { type: "action", action: "tasks.from-message" },
      { type: "action", action: "rules.create-from-message" },
    ],
  },
];

const item = (action: string) => ({ type: "action", action });
const MESSAGE_GROUP = [
  { type: "header", label: "@toolbar.group.message" },
  item("mail.toggle-read"),
  item("junk.mark"),
  item("junk.not-junk"),
  item("sync.download-message"),
  item("gmail.apply-labels"),
  item("exchange.apply-categories"),
  item("exchange.classify"),
];
const TURN_INTO_GROUP = [
  { type: "header", label: "@toolbar.group.turn-into" },
  item("calendar.event-from-message"),
  item("tasks.from-message"),
  item("rules.create-from-message"),
];

/**
 * The right-click menus matching the bar: the same actions in the same
 * order and groups (the bar's icon buttons as one block), every action of
 * today's menus kept. WP165 writes them into `defaults/layout.kdl`.
 */
export const PROPOSAL_CONTEXT_MENUS = {
  "context:message-list.row": [
    item("mail.reply"),
    item("mail.reply-all"),
    item("mail.forward"),
    { type: "separator" },
    item("mail.archive"),
    item("mail.delete"),
    item("mail.move"),
    item("mail.toggle-flag"),
    item("snooze.open"),
    { type: "separator" },
    ...MESSAGE_GROUP,
    item("print.message"),
    item("print.thread"),
    { type: "separator" },
    ...TURN_INTO_GROUP,
  ],
  "context:message-view": [
    item("mail.reply"),
    item("mail.reply-all"),
    item("mail.forward"),
    { type: "separator" },
    item("mail.archive"),
    item("mail.delete"),
    item("mail.move"),
    item("mail.toggle-flag"),
    item("snooze.open"),
    { type: "separator" },
    ...MESSAGE_GROUP,
    item("security.autocrypt.import"),
    item("print.message"),
    item("print.save-pdf"),
    { type: "separator" },
    ...TURN_INTO_GROUP,
  ],
};
