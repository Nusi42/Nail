/**
 * The portability rules' ports over the vectors generated from
 * `nail_api::{portability, backup, diagnostics}` (`portability.json`).
 */
import type { ExportFormat, ExportSource } from "../../api/generated";
import portabilityVectors from "../../../../crates/nail-api/fixtures/ts-vectors/portability.json";
import { describeVectors } from "../../test/vectors";
import {
  backupContentsIssues,
  backupDefaultOn,
  diagnosticsIsMail,
  diagnosticsIssues,
  exportAccepts,
  exportExtension,
  exportIsDirectory,
  importHasMail,
  importHasPim,
  importIsDirectory,
  isFinished,
  passphraseIssues,
  progressFraction,
} from "./model";

describeVectors("portability", portabilityVectors, {
  importIsDirectory,
  importHasMail,
  importHasPim,
  exportIsDirectory,
  exportExtension,
  exportAccepts: (format: ExportFormat, source: ExportSource["type"]) =>
    exportAccepts(format, source),
  backupDefaultOn,
  diagnosticsIsMail,
  isFinished,
  backupContentsIssues,
  passphraseIssues,
  diagnosticsIssues,
  progressFraction,
});
