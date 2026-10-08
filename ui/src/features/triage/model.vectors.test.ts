/** The triage settings checks' ports over the vectors generated from `nail_api::triage` (`triage.json`). */
import triageVectors from "../../../../crates/nail-api/fixtures/ts-vectors/triage.json";
import { describeVectors } from "../../test/vectors";
import { junkIssues, notificationIssues, parseHhmm } from "./model";

describeVectors("triage", triageVectors, { junkIssues, parseHhmm, notificationIssues });
