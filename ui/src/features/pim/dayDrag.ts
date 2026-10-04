/**
 * Dragging an event to another day (month view, all-day row): the day under
 * the pointer is the element with `data-day` there; dropping moves the
 * occurrence by the days between the two (asking the scope for recurring
 * events).
 */
import { useRef, useState, type PointerEvent as ReactPointerEvent } from "react";
import type { EventInstance } from "../../api/generated";
import { movedTimes, requestMove } from "./changes";
import { useChangeContext } from "./common";
import { parseDay } from "./time";

interface DayDrag {
  instance: EventInstance;
  from: string;
  over: string;
  moved: boolean;
}

/** The `data-day` under a viewport point, if any. */
export function dayAt(x: number, y: number): string | null {
  if (typeof document.elementFromPoint !== "function") return null;
  const element = document.elementFromPoint(x, y);
  return element?.closest("[data-day]")?.getAttribute("data-day") ?? null;
}

/** Days between two `YYYY-MM-DD`. */
export function daysBetween(from: string, to: string): number {
  const a = parseDay(from);
  const b = parseDay(to);
  return a && b ? b.compare(a) : 0;
}

export function useDayDrag(tz: string) {
  const ctx = useChangeContext();
  const [drag, setDrag] = useState<DayDrag | null>(null);
  const justDragged = useRef(false);

  return {
    /** The day a dragged event would land on. */
    over: drag?.moved ? drag.over : null,
    dragging: drag?.moved ? drag.instance : null,
    start: (event: ReactPointerEvent<HTMLElement>, instance: EventInstance, from: string) => {
      if (event.button !== 0 || instance.read_only) return;
      event.currentTarget.setPointerCapture(event.pointerId);
      setDrag({ instance, from, over: from, moved: false });
    },
    move: (event: ReactPointerEvent<HTMLElement>) => {
      if (!drag) return;
      const over = dayAt(event.clientX, event.clientY);
      if (over && over !== drag.over) setDrag({ ...drag, over, moved: true });
    },
    end: () => {
      if (!drag) return;
      const finished = drag;
      setDrag(null);
      if (!finished.moved) return;
      const days = daysBetween(finished.from, finished.over);
      if (days === 0) return;
      justDragged.current = true;
      const { start, end } = movedTimes(finished.instance, days, 0, tz);
      void requestMove(ctx, finished.instance, start, end);
    },
    cancel: () => {
      setDrag(null);
    },
    suppressClick: () => {
      if (!justDragged.current) return false;
      justDragged.current = false;
      return true;
    },
  };
}
