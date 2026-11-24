/**
 * The one footer of every editor, wizard and confirmation (R6, ADR 0239).
 * A dialog names its buttons by role; the footer orders them for the
 * viewer's system (`./order.ts`): Windows and Linux primary, secondary,
 * Cancel; macOS Cancel, secondary, primary — right-aligned in
 * `dialog.footer.end`. A dialog may have several secondary buttons (R8,
 * a plug-in's dialog): they keep the secondary place as one group, in the
 * order given on Windows and Linux and mirrored on macOS
 * (`secondaryOrder`). An editor's `Delete…` sits alone at the start
 * (`dialog.footer.start`). A finished task shows only Done (`done`).
 *
 * - Labels are verbs ("Save", "Create", "Delete folder"); Cancel is
 *   "Cancel" unless the dialog names its safe choice ("Keep meeting").
 * - Cancel goes the way × and Esc go (`close`, through the window's close
 *   guard, `../../layout/closeGuard.ts`), so it never commits.
 * - A destructive primary (`destructive`) is red and never a submit
 *   button: Enter never runs it. A normal primary may be the form's submit
 *   button (`submit`), so Enter in a single-line field runs it.
 * - `busy`: the action runs — every button but Cancel is disabled and the
 *   footer is `aria-busy`.
 *
 * Parts: `dialog.footer` (`data-os`, `data-state="done"` on a finished
 * task), `dialog.footer.start`, `dialog.footer.end`; each button is a
 * `form.button` with `data-role` (`primary` / `secondary` / `cancel` /
 * `start` / `done`), `data-variant` (`primary` / `danger`) and the
 * dialog's own `data-action`.
 */
import { Fragment, type ReactNode } from "react";
import { useT } from "../../i18n/I18nProvider";
import { useDialogOs } from "./dialogOs";
import { footerOrder, secondaryOrder, type FooterRole } from "./order";

/** A button of the footer. */
export interface FooterButton {
  /** The verb, localized. */
  label: string;
  onClick?: () => void;
  disabled?: boolean;
  /** It loses something: red, never a submit button, never the default. */
  destructive?: boolean;
  /**
   * The form's submit button (the primary of a form dialog): Enter in a
   * single-line field runs it. Ignored for a destructive button.
   */
  submit?: boolean;
  /** `data-action` (tests and themes tell the buttons apart by it). */
  action?: string;
  /**
   * Marked unavailable but still focusable (`aria-disabled`): pressing it
   * does nothing useful, and `describedBy` says why (a problem badge).
   */
  ariaDisabled?: boolean;
  /** `aria-describedby`: the id of what explains the button's state. */
  describedBy?: string;
}

/** Cancel: the label defaults to "Cancel"; `onClick` is the dialog's close. */
export interface FooterCancel {
  onClick: () => void;
  label?: string;
  disabled?: boolean;
  action?: string;
}

export interface DialogFooterProps {
  primary?: FooterButton;
  /** One secondary button, or several (one group in the secondary place). */
  secondary?: FooterButton | readonly FooterButton[];
  /** The cancel role (× and Esc do the same); left out only in a picker. */
  cancel?: FooterCancel;
  /** An editor's `Delete…`, alone at the start. */
  start?: FooterButton;
  /** The action runs: everything but Cancel is disabled. */
  busy?: boolean;
  /**
   * The task is finished: only Done shows (`onDone` closes the dialog);
   * every other button is left out.
   */
  done?: { onDone: () => void; label?: string };
  /** Content between the start and the end group (a step count). */
  children?: ReactNode;
}

function Button({
  kind,
  button,
  busy,
}: {
  kind: FooterRole | "start";
  button: FooterButton;
  busy: boolean;
}) {
  const destructive = button.destructive === true;
  const submit = button.submit === true && !destructive;
  return (
    <button
      type={submit ? "submit" : "button"}
      data-part="form.button"
      data-role={kind}
      data-variant={destructive ? "danger" : kind === "primary" ? "primary" : undefined}
      data-action={button.action}
      disabled={busy || button.disabled === true}
      aria-disabled={button.ariaDisabled === true || undefined}
      aria-describedby={button.describedBy}
      onClick={button.onClick}
    >
      {button.label}
    </button>
  );
}

/** One secondary button or several, as a list. */
function secondaryList(secondary: DialogFooterProps["secondary"]): readonly FooterButton[] {
  if (secondary === undefined) return [];
  return isButtonList(secondary) ? secondary : [secondary];
}

function isButtonList(
  secondary: FooterButton | readonly FooterButton[],
): secondary is readonly FooterButton[] {
  return Array.isArray(secondary);
}

/** The footer of a dialog: its buttons by role, in the order of the viewer's system. */
export function DialogFooter({
  primary,
  secondary,
  cancel,
  start,
  busy = false,
  done,
  children,
}: DialogFooterProps) {
  const t = useT();
  const os = useDialogOs();
  if (done !== undefined) {
    return (
      <div data-part="dialog.footer" data-os={os} data-state="done">
        {children}
        <div data-part="dialog.footer.end">
          <Button
            kind="done"
            button={{ label: done.label ?? t("common-done"), onClick: done.onDone, action: "done" }}
            busy={false}
          />
        </div>
      </div>
    );
  }
  const secondaries = secondaryOrder(os, secondaryList(secondary));
  const byRole: Partial<Record<FooterRole, ReactNode>> = {
    primary: primary && <Button kind="primary" button={primary} busy={busy} />,
    secondary: secondaries.map((button, index) => (
      <Button key={button.action ?? index} kind="secondary" button={button} busy={busy} />
    )),
    cancel: cancel && (
      <Button
        kind="cancel"
        button={{
          label: cancel.label ?? t("common-cancel"),
          onClick: cancel.onClick,
          ...(cancel.disabled === undefined ? {} : { disabled: cancel.disabled }),
          ...(cancel.action === undefined ? {} : { action: cancel.action }),
        }}
        busy={false}
      />
    ),
  };
  return (
    <div data-part="dialog.footer" data-os={os} aria-busy={busy || undefined}>
      {start && (
        <div data-part="dialog.footer.start">
          <Button kind="start" button={start} busy={busy} />
        </div>
      )}
      {children}
      <div data-part="dialog.footer.end">
        {footerOrder(os).map((role) => (
          <Fragment key={role}>{byRole[role]}</Fragment>
        ))}
      </div>
    </div>
  );
}
