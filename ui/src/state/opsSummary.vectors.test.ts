/** `opsWaiting` over the vectors generated from `nail_api::OpsSummary::waiting` (`ops.json`). */
import opsVectors from "../../../crates/nail-api/fixtures/ts-vectors/ops.json";
import { describeVectors } from "../test/vectors";
import { opsWaiting } from "./opsSummary";

describeVectors("ops", opsVectors, { opsWaiting });
