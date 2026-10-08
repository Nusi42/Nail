/**
 * The recovery-key, smartcard and sender-authentication ports over the
 * vectors generated from `nail_api::recovery` / `nail_api::smartcard` /
 * `AuthResults::summary` (`security.json`).
 */
import securityVectors from "../../../../crates/nail-api/fixtures/ts-vectors/security.json";
import { describeVectors } from "../../test/vectors";
import { authSummary } from "./model";
import { formatRecoveryKey, parseRecoveryKey } from "./recoveryKeyText";
import { pinIsPlausible, smartcardSettingsIssue } from "./smartcardModel";

describeVectors("security", securityVectors, {
  formatRecoveryKey: (bytes: number[]) => formatRecoveryKey(Uint8Array.from(bytes)),
  parseRecoveryKey: (text: string) => {
    const parsed = parseRecoveryKey(text);
    return parsed.ok ? { ok: true, bytes: [...parsed.bytes] } : parsed;
  },
  smartcardSettingsIssue,
  pinIsPlausible,
  authSummary,
});
