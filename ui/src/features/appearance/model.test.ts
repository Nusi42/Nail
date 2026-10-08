/**
 * The appearance rules' ports over the vectors generated from
 * `nail_api::appearance` / `nail_api::editing` (`appearance.json`) and from
 * nail-config's theme ids (`theme-id.json`).
 */
import type { SystemAppearance } from "../../api/generated";
import appearanceVectors from "../../../../crates/nail-api/fixtures/ts-vectors/appearance.json";
import themeIdVectors from "../../../../crates/nail-api/fixtures/ts-vectors/theme-id.json";
import { describeVectors } from "../../test/vectors";
import {
  accentIgnored,
  activeTheme,
  isHexColor,
  isThemeId,
  isThemeName,
  overrideIssues,
  themeIdFromName,
} from "./model";

describeVectors("appearance", appearanceVectors, {
  isHexColor,
  isThemeId,
  isThemeName,
  overrideIssues,
  activeTheme: (
    settings: Parameters<typeof activeTheme>[0],
    system: SystemAppearance | null,
    themes: Parameters<typeof activeTheme>[2],
  ) => activeTheme(settings, system ?? undefined, themes),
  accentIgnored,
});

describeVectors("theme ids", themeIdVectors, { themeIdFromName });
