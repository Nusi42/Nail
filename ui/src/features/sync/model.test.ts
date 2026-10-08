/** The sync policy ports over the vectors generated from the core's `sync_policy` module (`sync.json`). */
import type { AccountConfig, BodyPolicy } from "../../api/generated";
import syncVectors from "../../../../crates/nail-api/fixtures/ts-vectors/sync.json";
import { describeVectors } from "../../test/vectors";
import { resolvePolicy, withFolderRule } from "./model";

describeVectors("sync policies", syncVectors, {
  resolvePolicy,
  withFolderRule: (
    account: AccountConfig,
    path: string,
    bodies: BodyPolicy | null,
    enabled: boolean | null,
    idle: boolean | null,
  ) => withFolderRule(account, path, bodies, enabled, idle),
});
