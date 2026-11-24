/**
 * The order of a dialog's footer buttons (R6, ADR 0239): code names each
 * button's role (primary, secondary, cancel), never its place. Windows and
 * Linux put the primary first — primary, secondary, cancel from left to
 * right (WinUI's ContentDialog, Windows Settings, Office) — macOS puts it
 * last — cancel, secondary, primary (Apple's HIG: the default button at the
 * far right). Web mode follows the viewer's system, so the system is read
 * from the browser, not from the core (`PlatformInfo.os` is the server's
 * system in `nail serve`).
 *
 * Pure (no React): the end-to-end helpers check footers against the same
 * order (`e2e/support/windowTour.ts`). The React side is `./dialogOs.ts`.
 */
import type { Os } from "../../api/generated";

/** What a footer button does. `done` ends a finished task (alone). */
export type FooterRole = "primary" | "secondary" | "cancel" | "done";

/** Windows, Linux and the rest: the primary first, Cancel last. */
const PRIMARY_FIRST: readonly FooterRole[] = ["primary", "secondary", "cancel"];
/** macOS: the mirror image. */
const PRIMARY_LAST: readonly FooterRole[] = PRIMARY_FIRST.toReversed();

/** The roles of a footer's end group, left to right, on `os`. */
export function footerOrder(os: Os): readonly FooterRole[] {
  return os === "macos" ? PRIMARY_LAST : PRIMARY_FIRST;
}

/**
 * Several secondary buttons (R8), left to right on `os`: as given on
 * Windows and Linux, mirrored on macOS like the whole end group.
 */
export function secondaryOrder<T>(os: Os, buttons: readonly T[]): readonly T[] {
  return os === "macos" ? buttons.toReversed() : buttons;
}

/** What the browser says about its system (`navigator` or a stand-in). */
export interface NavigatorFacts {
  userAgentData?: { platform?: string } | undefined;
  platform?: string | undefined;
  userAgent?: string | undefined;
}

/** A system named in `text`, if any. */
function osIn(text: string): Os | null {
  if (/mac|iphone|ipad/i.test(text)) return "macos";
  if (/win/i.test(text)) return "windows";
  if (/linux|x11|android/i.test(text)) return "linux";
  return null;
}

/**
 * The viewer's system from what its browser reports, the most precise
 * source first (client hints, then `platform`, then the user agent);
 * `other` when none names one.
 */
export function osOfNavigator(nav: NavigatorFacts | undefined): Os {
  for (const said of [nav?.userAgentData?.platform, nav?.platform, nav?.userAgent]) {
    const os = said === undefined ? null : osIn(said);
    if (os !== null) return os;
  }
  return "other";
}

/** The system of the viewer of this page. */
export function viewerOs(): Os {
  return osOfNavigator(typeof navigator === "undefined" ? undefined : navigator);
}
