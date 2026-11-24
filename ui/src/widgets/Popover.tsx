/**
 * A non-modal popover anchored to a rectangle on screen (R7 contract, ADR
 * 0246): the calendar's quick look next to an event, the inline title
 * form after drawing a new event, search results under a field. The window
 * layer only centres windows on an anchor; a popover sits beside or below
 * it, inside the viewport.
 *
 * Behaviour: it renders into `document.body` (fixed position, so a size
 * container or a scrolling pane never clips it), takes the focus on open
 * (the first element marked `data-autofocus`, else the popover itself),
 * closes on Escape and on a pointer press outside it, and gives the focus
 * back to where it was on close. Position comes from a geometry rule
 * (`useGeometry`), never inline styles; the look from the theme part
 * (`popover` by default, `data-placement` beside / below).
 */
import {
  useEffect,
  useEffectEvent,
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ReactNode,
  type RefObject,
} from "react";
import { createPortal } from "react-dom";
import { useGeometry } from "../theme/ThemeProvider";

/** A rectangle in viewport coordinates (CSS pixels). */
export interface AnchorRect {
  left: number;
  top: number;
  width: number;
  height: number;
}

/** Where a popover goes relative to its anchor. */
export type PopoverPlacement = "below" | "beside";

/** Space between the anchor and the popover (CSS pixels). */
export const POPOVER_GAP = 4;

/** The viewport rectangle of an element (the anchor of a popover). */
export function anchorOf(element: Element): AnchorRect {
  const r = element.getBoundingClientRect();
  return { left: r.left, top: r.top, width: r.width, height: r.height };
}

/**
 * The top-left corner of a popover of `size` next to `anchor`, inside
 * `viewport`:
 *
 * - `below`: aligned with the anchor's start, under it; above it when there
 *   is no room below.
 * - `beside`: right of the anchor, top-aligned; left of it when there is no
 *   room on the right.
 *
 * Either way the popover is moved back into the viewport (never past its
 * top-left corner).
 */
export function placePopover(
  anchor: AnchorRect,
  size: { width: number; height: number },
  viewport: { width: number; height: number },
  placement: PopoverPlacement,
  gap = POPOVER_GAP,
): { x: number; y: number } {
  const clamp = (value: number, room: number) => Math.max(0, Math.min(value, room));
  const right = anchor.left + anchor.width;
  const bottom = anchor.top + anchor.height;
  if (placement === "below") {
    const below = bottom + gap;
    const y =
      below + size.height > viewport.height ? Math.max(0, anchor.top - size.height - gap) : below;
    return { x: clamp(anchor.left, viewport.width - size.width), y };
  }
  const after = right + gap;
  const x =
    after + size.width > viewport.width ? Math.max(0, anchor.left - size.width - gap) : after;
  return { x, y: clamp(anchor.top, viewport.height - size.height) };
}

/**
 * Keeps the element of `ref` at the place `placePopover` computes next to
 * `anchor` while `open` (`null` = not shown), through a geometry rule.
 * Returns the rule's id (the element's `data-geom`).
 */
export function usePopoverPosition(
  anchor: AnchorRect | null,
  ref: RefObject<HTMLElement | null>,
  placement: PopoverPlacement,
): string {
  const id = useId();
  const geom = `popover-${id.replace(/[^a-zA-Z0-9_-]/g, "_")}`;
  const [position, setPosition] = useState({ x: 0, y: 0 });
  useGeometry(geom, { left: `${String(position.x)}px`, top: `${String(position.y)}px` });
  const key = anchor === null ? null : JSON.stringify(anchor);
  useLayoutEffect(() => {
    if (key === null) return;
    const rect = JSON.parse(key) as AnchorRect;
    const place = () => {
      const box = ref.current?.getBoundingClientRect();
      const next = placePopover(
        rect,
        { width: box?.width ?? 0, height: box?.height ?? 0 },
        { width: window.innerWidth, height: window.innerHeight },
        placement,
      );
      setPosition((p) => (p.x === next.x && p.y === next.y ? p : next));
    };
    place();
    // The content can grow after opening (the quick look's details arrive
    // later), and the window can shrink: place it again on either.
    const element = ref.current;
    const observer =
      element !== null && typeof ResizeObserver !== "undefined" ? new ResizeObserver(place) : null;
    if (element !== null) observer?.observe(element);
    window.addEventListener("resize", place);
    return () => {
      observer?.disconnect();
      window.removeEventListener("resize", place);
    };
  }, [key, ref, placement]);
  return geom;
}

export interface PopoverProps {
  /** What the popover is next to. */
  anchor: AnchorRect;
  /** Beside (quick look) or below (fields, menus); default beside. */
  placement?: PopoverPlacement;
  /** Accessible name (localized). */
  label: string;
  /** Theme part (default `popover`). */
  part?: string;
  /** Closes it (Escape, a press outside); the caller drops it. */
  onClose: () => void;
  children: ReactNode;
}

/** A popover next to `anchor` (see the module comment). */
export function Popover({
  anchor,
  placement = "beside",
  label,
  part = "popover",
  onClose,
  children,
}: PopoverProps) {
  const ref = useRef<HTMLDivElement>(null);
  const anchorKey = JSON.stringify(anchor);
  const geom = usePopoverPosition(anchor, ref, placement);
  const close = useEffectEvent(onClose);

  // Focus on open; give it back on close.
  useEffect(() => {
    const before = document.activeElement;
    const first = ref.current?.querySelector<HTMLElement>("[data-autofocus]");
    (first ?? ref.current)?.focus();
    return () => {
      if (before instanceof HTMLElement && before.isConnected) before.focus();
    };
  }, [anchorKey]);

  // A press outside closes it, and so does Escape inside it.
  useEffect(() => {
    const onPointerDown = (event: PointerEvent) => {
      const target = event.target;
      if (target instanceof Node && ref.current?.contains(target)) return;
      close();
    };
    const onKeyDown = (event: globalThis.KeyboardEvent) => {
      const target = event.target;
      if (event.key !== "Escape" || !(target instanceof Node) || !ref.current?.contains(target)) {
        return;
      }
      event.preventDefault();
      event.stopPropagation();
      close();
    };
    document.addEventListener("pointerdown", onPointerDown, true);
    document.addEventListener("keydown", onKeyDown, true);
    return () => {
      document.removeEventListener("pointerdown", onPointerDown, true);
      document.removeEventListener("keydown", onKeyDown, true);
    };
  }, []);

  return createPortal(
    <div
      ref={ref}
      role="dialog"
      aria-label={label}
      tabIndex={-1}
      data-part={part}
      data-placement={placement}
      data-geom={geom}
    >
      {children}
    </div>,
    document.body,
  );
}
