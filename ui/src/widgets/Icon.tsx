import { useTheme } from "../theme/ThemeProvider";

export interface IconProps {
  /** Icon name from the theme's `icons`. */
  name: string | null | undefined;
  /** Part name of the `<svg>`. */
  part: string;
}

/** Renders a theme icon, or nothing when the theme has no such icon. */
export function Icon({ name, part }: IconProps) {
  const theme = useTheme();
  const icon = name && Object.hasOwn(theme.icons, name) ? theme.icons[name] : undefined;
  if (!icon) return null;
  return (
    <svg
      data-part={part}
      data-icon={name}
      viewBox={icon.view_box}
      aria-hidden="true"
      focusable="false"
      fill="currentColor"
    >
      <path d={icon.path} />
    </svg>
  );
}
