/**
 * Feature state of the customization slot (Zustand, outside `uiStore`):
 * every error of every configuration file, and the inspector.
 *
 * Config errors arrive as `ConfigErrors {file, errors}` (M1b: all errors of
 * a file, an empty list = valid again). The M1a event `ConfigError` (first
 * error only) is still honoured for a file until its first `ConfigErrors`
 * arrives; such legacy entries are cleared by `ConfigChanged` (theme,
 * layout), `AccountsChanged` and `RulesChanged`, as before.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { ConfigFile, ConfigIssue, Event as NailEvent } from "../../api/generated";

/** The errors of one configuration file. */
export interface FileErrors {
  issues: ConfigIssue[];
  /** `true` when only the M1a first-error event reported them. */
  legacy: boolean;
}

/** A file the merge dialog opens with, already compared (M5b layout offer). */
export interface MergePrefill {
  file: ConfigFile;
  incoming: string;
}

export interface CustomizationState {
  /** Errors by file name (`theme.kdl`, …); files without errors are absent. */
  configErrors: Record<string, FileErrors>;
  /** Files that sent `ConfigErrors` at least once. */
  modernFiles: string[];
  /** Whether the inspector's element picker is on. */
  inspecting: boolean;
  /** The element the inspector shows (picked, or hovered while picking). */
  inspected: Element | null;
  /** `true` once the user clicked an element (the panel stays on it). */
  pinned: boolean;
  /** What the merge dialog opens with next (taken once by the dialog). */
  mergePrefill: MergePrefill | null;

  applyEvent(event: NailEvent): void;
  setInspecting(on: boolean): void;
  inspect(element: Element | null, pinned: boolean): void;
  prefillMerge(prefill: MergePrefill | null): void;
  /** Returns and clears the merge prefill. */
  takeMergePrefill(): MergePrefill | null;
  reset(): void;
}

const LEGACY_CLEARED_BY: Partial<Record<NailEvent["type"], readonly string[]>> = {
  ConfigChanged: ["theme.kdl", "layout.kdl"],
  AccountsChanged: ["accounts.kdl"],
  RulesChanged: ["rules.kdl"],
};

function initial() {
  return {
    configErrors: {} as Record<string, FileErrors>,
    modernFiles: [] as string[],
    inspecting: false,
    inspected: null as Element | null,
    pinned: false,
    mergePrefill: null as MergePrefill | null,
  };
}

function without(
  errors: Record<string, FileErrors>,
  drop: (file: string) => boolean,
): Record<string, FileErrors> {
  return Object.fromEntries(Object.entries(errors).filter(([file]) => !drop(file)));
}

/** The next error map after an event (pure; exported for tests). */
export function nextConfigErrors(
  current: Record<string, FileErrors>,
  modernFiles: readonly string[],
  event: NailEvent,
): { configErrors: Record<string, FileErrors>; modernFiles: string[] } | null {
  switch (event.type) {
    case "ConfigErrors": {
      const configErrors = without(current, (file) => file === event.file);
      if (event.errors.length > 0) {
        configErrors[event.file] = { issues: event.errors, legacy: false };
      }
      return {
        configErrors,
        modernFiles: modernFiles.includes(event.file)
          ? [...modernFiles]
          : [...modernFiles, event.file],
      };
    }
    case "ConfigError": {
      if (modernFiles.includes(event.file)) return null;
      return {
        configErrors: {
          ...current,
          [event.file]: {
            issues: [{ line: event.line, column: event.column, message: event.message }],
            legacy: true,
          },
        },
        modernFiles: [...modernFiles],
      };
    }
    default: {
      const files = LEGACY_CLEARED_BY[event.type];
      if (!files) return null;
      const cleared = (file: string) => files.includes(file) && current[file]?.legacy === true;
      if (!Object.keys(current).some(cleared)) return null;
      return { configErrors: without(current, cleared), modernFiles: [...modernFiles] };
    }
  }
}

export const customizationStore = createStore<CustomizationState>()((set, get) => ({
  ...initial(),
  applyEvent: (event) => {
    const next = nextConfigErrors(get().configErrors, get().modernFiles, event);
    if (next) set(next);
  },
  setInspecting: (inspecting) =>
    set(
      inspecting ? { inspecting, pinned: false } : { inspecting, inspected: null, pinned: false },
    ),
  inspect: (inspected, pinned) => set({ inspected, pinned }),
  prefillMerge: (mergePrefill) => set({ mergePrefill }),
  takeMergePrefill: () => {
    const prefill = get().mergePrefill;
    if (prefill) set({ mergePrefill: null });
    return prefill;
  },
  reset: () => set(initial()),
}));

/** Subscribes to a slice of the feature state. */
export function useCustomization<T>(selector: (state: CustomizationState) => T): T {
  return useStore(customizationStore, selector);
}

/** Every error of every file, files sorted by name. */
export function allConfigErrors(
  errors: Record<string, FileErrors>,
): { file: string; issues: ConfigIssue[] }[] {
  return Object.keys(errors)
    .sort()
    .map((file) => ({ file, issues: errors[file]?.issues ?? [] }));
}
