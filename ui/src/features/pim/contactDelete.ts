/**
 * Deleting contacts asks first (R6, ADR 0239 rule 5): the contact card's
 * Delete, the list's "Delete N" and its Delete key, and the contact
 * editor's Delete… open the modal question `pim-delete-contacts` ("Delete
 * the contact “Ann”?" / "Delete 3 contacts?"; a red verb, the focus on
 * Cancel). Only the verb deletes, through the core (`DeleteContacts`).
 */
import type { MessageArgs } from "../../i18n/localizer";
import type { ChangeContext } from "./changes";
import { askPim } from "./Confirm";
import { DELETE_CONTACTS_DIALOG } from "./names";
import { pimWrite } from "./queries";

/** What the question names and what happens after the deletion. */
export interface ContactsToDelete {
  ids: readonly string[];
  /** The name of the one contact (ignored for several). */
  name: string;
  /** After the deletion (clear the selection, close the editor). */
  onDeleted: () => void;
}

/** Asks before deleting `what.ids`; nothing happens for none. */
export function askDeleteContacts(
  ctx: ChangeContext,
  t: (id: string, args?: MessageArgs) => string,
  what: ContactsToDelete,
): void {
  const count = what.ids.length;
  if (count === 0) return;
  askPim(ctx.store, DELETE_CONTACTS_DIALOG, {
    question: t("pim-delete-contacts-question", { count, name: what.name }),
    action: t("pim-delete-contacts-action", { count }),
    run: async () => {
      await pimWrite(ctx, { type: "DeleteContacts", contact_ids: [...what.ids] }, "PimDeleted");
      what.onDeleted();
    },
  });
}
