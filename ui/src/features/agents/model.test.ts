/** The policy table's port over the vectors generated from `nail_api::agents` (`agents.json`). */
import agentsVectors from "../../../../crates/nail-api/fixtures/ts-vectors/agents.json";
import { describeVectors } from "../../test/vectors";
import { policyTable } from "./model";

describeVectors("agents", agentsVectors, { policyTable });
