/** The update-state rules' ports over the vectors generated from `nail_api::desktop` (`system.json`). */
import systemVectors from "../../../../crates/nail-api/fixtures/ts-vectors/system.json";
import { describeVectors } from "../../test/vectors";
import { canCheck, canDownload, canInstall, updateOf } from "./model";

describeVectors("desktop", systemVectors, { canCheck, canDownload, canInstall, updateOf });
