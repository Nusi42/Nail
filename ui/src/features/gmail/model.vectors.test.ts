/**
 * The label-settings check over the vectors generated from
 * `GmailLabelSettings::issues` (`gmail.json`); duplicates against other
 * labels are the UI's own addition (none here).
 */
import gmailVectors from "../../../../crates/nail-api/fixtures/ts-vectors/gmail.json";
import type { GmailLabelSettings } from "../../api/generated";
import { describeVectors } from "../../test/vectors";
import { labelIssues } from "./model";

describeVectors("gmail", gmailVectors, {
  labelIssues: (settings: GmailLabelSettings) => labelIssues(settings, []),
});
