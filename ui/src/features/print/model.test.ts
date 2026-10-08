/** The print rules' ports over the vectors generated from `nail_api::print` (`print.json`). */
import type { PrintTarget } from "../../api/generated";
import printVectors from "../../../../crates/nail-api/fixtures/ts-vectors/print.json";
import { describeVectors } from "../../test/vectors";
import { printViewRoute, targetIssue } from "./model";

describeVectors("print", printVectors, {
  targetIssue: (target: PrintTarget) => targetIssue(target),
  printViewRoute,
});
