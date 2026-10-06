/**
 * A check box of the label / category windows (P3 review): checked when
 * every target message has the tag, mixed (`indeterminate`) when some have
 * it; the user's choice wins once made.
 */
import type { TagMark } from "./tags";

export interface TagCheckboxProps {
  /** The tag on the targets' messages. */
  mark: TagMark;
  /** The user's choice, if any. */
  decided: boolean | undefined;
  onDecide: (checked: boolean) => void;
}

export function TagCheckbox({ mark, decided, onDecide }: TagCheckboxProps) {
  const mixed = decided === undefined && mark === "some";
  return (
    <input
      type="checkbox"
      checked={decided ?? mark === "all"}
      data-mixed={mixed ? "true" : undefined}
      ref={(element) => {
        if (element) element.indeterminate = mixed;
      }}
      onChange={(e) => {
        onDecide(e.target.checked);
      }}
    />
  );
}
