/**
 * Icons (M5a, ADR 0102). Resolution: the active theme's `icons` (a custom
 * SVG by name, so a theme can still replace or add any icon) → the bundled
 * Lucide icon (a name in `BUNDLED_ICONS`, or a pre-M5 Nail name such as
 * `compose` → `square-pen`) → nothing.
 *
 * Lucide icons draw with `currentColor`; their size comes from the theme
 * (`size.icon` through the part's width / height), never from the component.
 */
import { useTheme } from "../theme/ThemeProvider";
import { LEGACY_ICON_NAMES, LUCIDE_ICONS } from "./lucideIcons";

export interface IconProps {
  /** Lucide name (or a theme icon's name). */
  name: string | null | undefined;
  /** Part name of the `<svg>`. */
  part: string;
}

/** The Lucide name a layout / plug-in icon name stands for, if bundled. */
export function lucideName(name: string): string | null {
  if (Object.hasOwn(LUCIDE_ICONS, name)) return name;
  const legacy = Object.hasOwn(LEGACY_ICON_NAMES, name) ? LEGACY_ICON_NAMES[name] : undefined;
  return legacy !== undefined && Object.hasOwn(LUCIDE_ICONS, legacy) ? legacy : null;
}

/**
 * lucide-react always sets `class="lucide lucide-<name>"`; Nail styles only
 * by `data-part` and keeps the DOM free of class names (e2e `mail.spec.ts`),
 * so the attribute is removed when the `<svg>` mounts. React never writes it
 * again: the computed class name of an icon does not change between renders.
 */
function dropLucideClass(svg: SVGSVGElement | null): void {
  svg?.removeAttribute("class");
}

/** Renders a theme or Lucide icon, or nothing when neither has the name. */
export function Icon({ name, part }: IconProps) {
  const theme = useTheme();
  if (!name) return null;
  const custom = Object.hasOwn(theme.icons, name) ? theme.icons[name] : undefined;
  if (custom) {
    return (
      <svg
        data-part={part}
        data-icon={name}
        viewBox={custom.view_box}
        aria-hidden="true"
        focusable="false"
        fill="currentColor"
      >
        <path d={custom.path} />
      </svg>
    );
  }
  const lucide = lucideName(name);
  const Lucide = lucide === null ? undefined : LUCIDE_ICONS[lucide];
  if (!Lucide || lucide === null) return null;
  return (
    <Lucide
      ref={dropLucideClass}
      data-part={part}
      data-icon={lucide}
      data-icon-set="lucide"
      aria-hidden="true"
      focusable="false"
    />
  );
}
