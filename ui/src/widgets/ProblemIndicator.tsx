/**
 * The compact problem indicator (R1, ADR 0221): one widget for every place
 * where a mistake must be visible without pushing the content around — the
 * search field's query mistake and the rules window's errors and missing
 * pieces. It shows a small icon (and an optional short label such as
 * "2 errors"); its details float over the content instead of taking room.
 *
 * Two modes:
 * - **toggle** (no `open` prop): the indicator is a button. Its hover text is
 *   `title`; Enter / Space or a click opens the details as a native popover
 *   in the top layer (Escape and a click outside close it). The details are
 *   the button's description, so a screen reader reads them with it.
 * - **follow** (`open` given): the host decides when the details show (the
 *   search field: while the pointer is over the form or focus is in it). The
 *   icon is decorative; the details float under the nearest positioned
 *   ancestor (the host sets `position: relative`), stay in the DOM while
 *   hidden and carry `detailsId`, which the host's field names in
 *   `aria-describedby`.
 *
 * Parts: `problem` (`data-severity`, `data-kind`, `data-mode`), `problem.badge`,
 * `problem.icon`, `problem.label`, `problem.details` (same data attributes).
 * The look is the shared skin's (`theme/skin.ts`, `PROBLEM`).
 */
import { useId, type ReactNode } from "react";
import { Icon } from "./Icon";

/** `error`: something failed; `warning`: something is not ready yet. */
export type ProblemSeverity = "error" | "warning";

export interface ProblemIndicatorProps {
  severity: ProblemSeverity;
  /** What went wrong, as `data-kind` (tests and themes); the severity when omitted. */
  kind?: string | undefined;
  /** Short localized text beside the icon; icon only when omitted. */
  label?: string | undefined;
  /** Localized hover text of the button (toggle mode). */
  title?: string | undefined;
  /** Follow mode: the details show while this is true. */
  open?: boolean | undefined;
  /** The details' id (follow mode: the host's `aria-describedby`); generated when omitted. */
  detailsId?: string | undefined;
  /** The details, already localized. */
  children: ReactNode;
}

export function ProblemIndicator({
  severity,
  kind = severity,
  label,
  title,
  open,
  detailsId,
  children,
}: ProblemIndicatorProps) {
  const generated = useId();
  const id = detailsId ?? generated;
  const mode = open === undefined ? "toggle" : "follow";
  const data = { "data-severity": severity, "data-kind": kind, "data-mode": mode };
  const face = (
    <>
      <Icon name="circle-alert" part="problem.icon" />
      {label !== undefined && <span data-part="problem.label">{label}</span>}
    </>
  );
  if (open !== undefined) {
    return (
      <span data-part="problem" {...data}>
        <span data-part="problem.badge" aria-hidden="true">
          {face}
        </span>
        <div data-part="problem.details" {...data} id={id} role="tooltip" hidden={!open}>
          {children}
        </div>
      </span>
    );
  }
  return (
    <span data-part="problem" {...data} role={severity === "error" ? "alert" : undefined}>
      <button
        type="button"
        data-part="problem.badge"
        title={title}
        aria-describedby={id}
        popoverTarget={id}
      >
        {face}
      </button>
      <div data-part="problem.details" {...data} id={id} popover="auto">
        {children}
      </div>
    </span>
  );
}
