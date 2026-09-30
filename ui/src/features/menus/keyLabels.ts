/**
 * Key bindings as people read them (`ctrl+shift+m` → "Ctrl+Shift+M",
 * `g r` → "G then R") and as `aria-keyshortcuts` wants them. Modifier and
 * named-key texts come from the catalog (`context-menu-key-*`); other keys
 * are shown as typed, upper-cased.
 */
import { normalizeCombo } from "../../actions/keys";

/** Keys with a catalog text (`context-menu-key-<name>`). */
export const NAMED_KEYS = [
  "ctrl",
  "shift",
  "alt",
  "meta",
  "enter",
  "escape",
  "delete",
  "backspace",
  "tab",
  "space",
  "arrowup",
  "arrowdown",
  "arrowleft",
  "arrowright",
  "home",
  "end",
  "pageup",
  "pagedown",
  "contextmenu",
] as const;

const NAMED = new Set<string>(NAMED_KEYS);

type T = (id: string, args?: Record<string, string | number>) => string;

function keyText(key: string, t: T): string {
  const name = key === " " ? "space" : key;
  if (NAMED.has(name)) return t(`context-menu-key-${name}`);
  return name.length === 1 ? name.toUpperCase() : name.charAt(0).toUpperCase() + name.slice(1);
}

/** Modifiers and key of a chord (`ctrl++` → `["ctrl", "+"]`). */
function chordPieces(chord: string): string[] {
  const normalized = normalizeCombo(chord);
  if (normalized.endsWith("+")) {
    return [...normalized.slice(0, -1).split("+").filter(Boolean), "+"];
  }
  return normalized.split("+");
}

/** One chord (`ctrl+s`) as text. */
export function chordText(chord: string, t: T): string {
  return chordPieces(chord)
    .map((p) => keyText(p, t))
    .join("+");
}

/** A binding (chord or sequence) as text. */
export function keyLabel(binding: string, t: T): string {
  const steps = binding.trim().split(/\s+/).filter(Boolean);
  if (steps.length <= 1) return chordText(steps[0] ?? "", t);
  return t("context-menu-key-sequence", {
    keys: steps.map((s) => chordText(s, t)).join(" "),
  });
}

const ARIA_NAMES: Record<string, string> = {
  ctrl: "Control",
  shift: "Shift",
  alt: "Alt",
  meta: "Meta",
  " ": "Space",
};

/**
 * `aria-keyshortcuts` for a single chord (`Control+Shift+M`); `undefined`
 * for sequences, which the attribute cannot express.
 */
export function ariaShortcut(binding: string): string | undefined {
  const steps = binding.trim().split(/\s+/).filter(Boolean);
  if (steps.length !== 1) return undefined;
  return chordPieces(steps[0] as string)
    .map((p) => ARIA_NAMES[p] ?? p.charAt(0).toUpperCase() + p.slice(1))
    .join("+");
}
