/**
 * A badge button that opens a details panel (the popover of the message
 * security and sender badges). Positioning is the theme's job
 * (`<part>.popover`); the component handles focus and dismissal:
 * Escape and a click outside close it, focus returns to the badge.
 */
import { useEffect, useId, useRef, useState, type ReactNode } from "react";

export interface PopoverProps {
  /** Part name of the badge; the panel is `<part>.popover`. */
  part: string;
  /** Accessible name of the badge button and the panel. */
  label: string;
  /** Extra `data-*` attributes of the badge (tone, state). */
  data?: Record<string, string | undefined>;
  /** Badge content. */
  badge: ReactNode;
  /** Panel content (rendered only while open). */
  children: ReactNode;
}

export function Popover({ part, label, data = {}, badge, children }: PopoverProps) {
  const [open, setOpen] = useState(false);
  const id = useId();
  const root = useRef<HTMLSpanElement>(null);
  const button = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDown = (event: MouseEvent) => {
      if (root.current && !root.current.contains(event.target as Node)) setOpen(false);
    };
    document.addEventListener("mousedown", onDown);
    return () => {
      document.removeEventListener("mousedown", onDown);
    };
  }, [open]);

  const dataAttrs = Object.fromEntries(
    Object.entries(data).map(([k, v]) => [`data-${k}`, v] as const),
  );

  return (
    // eslint-disable-next-line jsx-a11y/no-static-element-interactions -- Escape from the button or the popover inside closes the popover.
    <span
      ref={root}
      data-part={`${part}.anchor`}
      onKeyDown={(event) => {
        if (event.key === "Escape" && open) {
          event.stopPropagation();
          setOpen(false);
          button.current?.focus();
        }
      }}
    >
      <button
        ref={button}
        type="button"
        data-part={part}
        data-state={open ? "open" : undefined}
        {...dataAttrs}
        aria-expanded={open}
        aria-controls={open ? id : undefined}
        aria-label={label}
        onClick={() => {
          setOpen((o) => !o);
        }}
      >
        {badge}
      </button>
      {open && (
        <div id={id} data-part={`${part}.popover`} role="dialog" aria-label={label}>
          {children}
        </div>
      )}
    </span>
  );
}
