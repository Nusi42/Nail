/**
 * The texts the plug-in shows at run time (English). They are the
 * plug-in's own data, not Nail's; the manifest's texts are in
 * locales/en.ftl.
 */
export const strings = {
  dialogTitle: "Follow up",
  remindAbout: (subject: string) => `Remind me about “${subject || "(no subject)"}”`,
  from: (sender: string) => `From ${sender}`,
  due: "Due",
  note: "Note",
  notePlaceholder: "What needs doing?",
  save: "Save reminder",
  remove: "Remove reminder",
  cancel: "Cancel",
  saved: (due: string) => `Follow-up reminder set for ${due}`,
  removed: "Follow-up reminder removed",
  noSelection: "Select a message to follow up on",
  notReadable: "This message cannot be read by the Follow up plug-in",
  notSaved: "The reminder could not be saved",
  panelEmpty: "No follow-ups. Use “Follow up…” on a message to add one.",
  panelOverdue: (count: number) => `${count} overdue`,
  columnDue: "Due",
  columnSubject: "Subject",
  columnNote: "Note",
  removePast: "Remove past reminders",
} as const;
