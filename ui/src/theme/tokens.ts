/**
 * Token shorthands of the skins (Q2, SSOT): every skin module writes its
 * rules with these `var(--nail-…)` references instead of keeping its own
 * map, so a token is named once. The values are the token schema's
 * (`defaults/theme.kdl`); a fallback stands only where a token is optional
 * in the schema (a theme may leave it out).
 */
export const T = {
  bg: "var(--nail-color-bg)",
  subtle: "var(--nail-color-bg-subtle)",
  raised: "var(--nail-color-bg-raised)",
  /** The window background behind panes (optional: the background). */
  canvas: "var(--nail-color-canvas, var(--nail-color-bg))",
  /** The navigation rail (optional: the subtle background). */
  rail: "var(--nail-color-rail, var(--nail-color-bg-subtle))",
  fg: "var(--nail-color-fg)",
  muted: "var(--nail-color-fg-muted)",
  border: "var(--nail-color-border)",
  /** Outlines of controls (inputs, selects, switches): 3:1 against surfaces. */
  line: "var(--nail-color-border-strong, var(--nail-color-border))",
  /** The whole outline of input controls (`border "control"`; 2px in High contrast). */
  controlBorder:
    "var(--nail-border-control, 1px solid var(--nail-color-border-strong, var(--nail-color-border)))",
  accent: "var(--nail-color-accent)",
  accentFg: "var(--nail-color-accent-fg)",
  /** Every theme has it (the base theme defines it for each mode). */
  accentSoft: "var(--nail-color-accent-soft)",
  selection: "var(--nail-color-selection)",
  selectionFg: "var(--nail-color-selection-fg)",
  danger: "var(--nail-color-danger)",
  warning: "var(--nail-color-warning)",
  success: "var(--nail-color-success)",
  focus: "var(--nail-color-focus-ring)",
  scrim: "var(--nail-color-scrim)",
  /** The bar beside quoted text (optional: the border colour). */
  quote: "var(--nail-color-quote-1, var(--nail-color-border))",
  fontUi: "var(--nail-font-ui)",
  fontMono: "var(--nail-font-mono)",
  fontDisplay: "var(--nail-font-display, var(--nail-font-ui))",
  fontReader: "var(--nail-font-reader, var(--nail-font-ui))",
  label: "var(--nail-font-size-label)",
  small: "var(--nail-font-size-small)",
  body: "var(--nail-font-size-body)",
  heading: "var(--nail-font-size-heading)",
  title: "var(--nail-font-size-title)",
  regular: "var(--nail-font-weight-regular)",
  medium: "var(--nail-font-weight-medium)",
  semibold: "var(--nail-font-weight-semibold)",
  tight: "var(--nail-line-height-tight)",
  normal: "var(--nail-line-height-normal)",
  relaxed: "var(--nail-line-height-relaxed)",
  xxs: "var(--nail-space-xxs)",
  xs: "var(--nail-space-xs)",
  sm: "var(--nail-space-sm)",
  md: "var(--nail-space-md)",
  lg: "var(--nail-space-lg)",
  xl: "var(--nail-space-xl)",
  xxl: "var(--nail-space-xxl)",
  pane: "var(--nail-space-pane)",
  gap: "var(--nail-space-gap)",
  section: "var(--nail-space-section)",
  row: "var(--nail-space-row)",
  icon: "var(--nail-size-icon)",
  control: "var(--nail-size-control)",
  avatar: "var(--nail-size-avatar)",
  /** The small avatar of the conversation strip (a skin variable of `threadViewSkin.ts`). */
  avatarSm: "var(--nail-skin-avatar-sm)",
  rSm: "var(--nail-radius-sm)",
  rMd: "var(--nail-radius-md)",
  rLg: "var(--nail-radius-lg)",
  pill: "var(--nail-radius-pill)",
  raisedShadow: "var(--nail-shadow-raised)",
  overlay: "var(--nail-shadow-overlay)",
  windowShadow: "var(--nail-shadow-window)",
  fast: "var(--nail-motion-fast)",
  ease: "var(--nail-motion-ease)",
  disabled: "var(--nail-opacity-disabled)",
} as const;
