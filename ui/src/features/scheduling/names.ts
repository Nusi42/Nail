/** Dialog and widget names of the scheduling feature. */

/** Cancel a meeting (whole / one occurrence / some attendees). */
export const CANCEL_DIALOG = "scheduling-cancel-meeting";
/** Automatic processing of invitations. */
export const SETTINGS_DIALOG = "scheduling-settings";
/** Find a time for several people and create the meeting. */
export const FIND_TIME_DIALOG = "scheduling-find-time";

export const DIALOGS = [CANCEL_DIALOG, SETTINGS_DIALOG, FIND_TIME_DIALOG] as const;

/** Layout widget kinds (registered in `nail-config/src/widgets/scheduling.rs`). */
export const INVITATION_CARD_WIDGET = "invitation-card";
export const FREEBUSY_WIDGET = "freebusy-timeline";
