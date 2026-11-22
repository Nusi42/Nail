/**
 * A dialog's form (R6, ADR 0239): Enter in a single-line field runs the
 * footer's submit button (the primary, when it is not destructive);
 * Ctrl+Enter (Cmd+Enter) runs it from a multi-line field too, where plain
 * Enter adds a line. The submit event never reloads the page.
 *
 * The dialog names its part (`part`, e.g. `theme-dialog`) and its
 * `data-dialog`.
 */
import type { KeyboardEvent, ReactNode, SubmitEvent } from "react";

/** Whether `event` is Ctrl+Enter / Cmd+Enter in a multi-line field. */
export function isMultilineSubmit(event: KeyboardEvent<HTMLElement>): boolean {
  return (
    event.key === "Enter" &&
    (event.ctrlKey || event.metaKey) &&
    !event.altKey &&
    event.target instanceof HTMLTextAreaElement
  );
}

export function DialogForm({
  part,
  dialog,
  busy = false,
  onSubmit,
  children,
}: {
  part: string;
  /** `data-dialog`. */
  dialog?: string;
  busy?: boolean;
  onSubmit: () => void;
  children: ReactNode;
}) {
  return (
    // eslint-disable-next-line jsx-a11y/no-noninteractive-element-interactions -- Ctrl+Enter in a multi-line field submits the form, like Enter in a single-line field.
    <form
      data-part={part}
      data-dialog={dialog}
      aria-busy={busy || undefined}
      noValidate
      onSubmit={(event: SubmitEvent<HTMLFormElement>) => {
        event.preventDefault();
        onSubmit();
      }}
      onKeyDown={(event) => {
        if (!isMultilineSubmit(event)) return;
        event.preventDefault();
        // Like Enter in a single-line field: only through an enabled submit button.
        const submit =
          event.currentTarget.querySelector<HTMLButtonElement>('button[type="submit"]');
        if (submit !== null && !submit.disabled) event.currentTarget.requestSubmit(submit);
      }}
    >
      {children}
    </form>
  );
}
