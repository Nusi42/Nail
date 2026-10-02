/**
 * Names this feature registers: the print dialog (window title
 * `window-print` in `print.ftl`) and the action ids. Toolbars, menus and
 * keys of `layout.kdl` refer to the actions (WP125 adds them to the
 * defaults).
 */

/** Dialog: what to print and how (paper, headers, …), then Print / Save as PDF. */
export const PRINT_DIALOG = "print";

/** Action: prints the selected message. */
export const PRINT_MESSAGE_ACTION = "print.message";
/** Action: prints the selected message's whole conversation. */
export const PRINT_THREAD_ACTION = "print.thread";
/** Action: prints the selected event (or the event id given as argument). */
export const PRINT_EVENT_ACTION = "print.event";
/** Action: prints an agenda (a date range of the calendars). */
export const PRINT_AGENDA_ACTION = "print.agenda";
/** Action: saves the selected message as a PDF file (desktop). */
export const PRINT_SAVE_PDF_ACTION = "print.save-pdf";
