/**
 * The dialog registry: named dialogs contributed by features
 * (`src/features/<name>/index.ts`). `window.open "<name>"` opens one when
 * the layout has no `window "<name>"`; it is shown in the same modal window
 * frame as layout windows (title `window-<name>`).
 */
import { createContext, useContext, type ComponentType } from "react";

export interface DialogProps {
  /** The dialog's name (its window name). */
  name: string;
  /** Closes the dialog. */
  close: () => void;
}

export type DialogRegistry = Readonly<Record<string, ComponentType<DialogProps>>>;

export const DialogRegistryContext = createContext<DialogRegistry>({});

export function useDialogRegistry(): DialogRegistry {
  return useContext(DialogRegistryContext);
}
