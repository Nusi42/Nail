/** The sieve settings checks' ports over the vectors generated from `nail_api::sieve` (`sieve.json`). */
import sieveVectors from "../../../../crates/nail-api/fixtures/ts-vectors/sieve.json";
import { describeVectors } from "../../test/vectors";
import { isIsoDate, manageSieveIssues, vacationIssues } from "./model";

describeVectors("sieve", sieveVectors, { isIsoDate, vacationIssues, manageSieveIssues });
