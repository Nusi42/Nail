/**
 * The quick look (R7 M4, ADR 0246): a popover next to the event a click
 * opened (`PimState.quickLook`), answering when, where, who and how to join
 * without leaving the view; Enter opens the full editor, Esc closes it.
 *
 * Contract slot: the host is mounted once (the feature's `effects`) and
 * shows the shared `Popover` with the event's title. The quick-look
 * package fills in the content (time, calendar, place, people, RSVP,
 * Edit / Delete / Join) with the `pim.popover.*` parts and the texts of
 * `locales/en/calendar-look.ftl`.
 */
import { Popover } from "../../widgets/Popover";
import { usePim, usePimStore } from "./store";

/** Shows the open quick look, if any. */
export function EventPopoverHost() {
  const quickLook = usePim((s) => s.quickLook);
  const pim = usePimStore();
  if (quickLook === null) return null;
  const { instance, anchor } = quickLook;
  return (
    <Popover
      anchor={anchor}
      label={instance.title}
      part="pim.popover"
      onClose={() => {
        pim.getState().setQuickLook(null);
      }}
    >
      <h2 data-part="pim.popover.title">{instance.title}</h2>
    </Popover>
  );
}
