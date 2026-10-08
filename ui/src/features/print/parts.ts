/**
 * Structure of printing every theme needs (the "print variant", ADR 0094):
 *
 * - on screen the print view covers the app (it is a page preview with a
 *   toolbar);
 * - when printing only the print view is printed: everything else in the
 *   document is hidden, the toolbar too;
 * - `@page` rules for every paper size and orientation, used through the
 *   view's `data-page` (CSS named pages);
 * - message text keeps its line breaks; agendas break pages per day
 *   (`day` layout) and per week (`week`).
 *
 * Colours, fonts and spacing come from the theme's `print.*` parts and its
 * `print` mode tokens. The integrator appends this to the compiled theme's
 * base CSS (`ui/src/theme/compile.ts`).
 */
import { pageRules } from "./model";
import { INLINE_GAP } from "../../theme/declarations";

export const PRINT_BASE_CSS = `[data-part="print"] { position: fixed; inset: 0; z-index: 2000; overflow: auto; }
[data-part="print.toolbar"] { display: flex; flex-wrap: wrap; gap: ${INLINE_GAP}; }
[data-part="print.text"], [data-part="print.quote"], [data-part="print.body"] { white-space: pre-wrap; overflow-wrap: anywhere; }
[data-part="print.headers"] { margin: 0; }
[data-part="print.header"] { display: flex; gap: ${INLINE_GAP}; }
[data-part="print.header.value"] { margin: 0; overflow-wrap: anywhere; }
[data-part="print.attachments.list"], [data-part="print.attendees.list"], [data-part="print.agenda.events"] { list-style: none; margin: 0; padding: 0; }
[data-part="print.message"] + [data-part="print.message"] { break-before: page; }
[data-part="print.agenda.month"] { width: 100%; border-collapse: collapse; table-layout: fixed; break-after: page; }
[data-part="print.agenda.cell"] { vertical-align: top; }
[data-part="print.agenda"][data-layout="day"] [data-part="print.agenda.day"] { break-after: page; }
[data-part="print.agenda"][data-layout="week"] [data-part="print.agenda.group"] { break-after: page; }
[data-part="print.agenda.day"], [data-part="print.agenda.event"], [data-part="print.header"] { break-inside: avoid; }
@media print {
  body > :not([data-part="print"]) { display: none !important; }
  [data-part="print"] { position: static; overflow: visible; }
  [data-part="print.toolbar"] { display: none; }
  [data-part="print"]:not([data-backgrounds]) [data-part="print.page"] { print-color-adjust: economy; }
}
${pageRules()}
`;
