/**
 * The busy / problem state of a form or dialog that runs one write at a
 * time: `run` marks it busy, clears the last problem, awaits the write and
 * keeps the core's localized refusal as the problem when it fails.
 */
import { useState } from "react";
import { errorMessageOf } from "../api/client";
import type { LocalizedMessage } from "../api/generated";

export interface BusyAction {
  busy: boolean;
  /** The last failure (or a problem the form set itself), `null` while none. */
  problem: LocalizedMessage | null;
  setProblem: (problem: LocalizedMessage | null) => void;
  /** Runs `write`; resolves `true` when it succeeded, `false` when it threw. */
  run: (write: () => Promise<void>) => Promise<boolean>;
}

export function useBusyAction(): BusyAction {
  const [busy, setBusy] = useState(false);
  const [problem, setProblem] = useState<LocalizedMessage | null>(null);
  const run = async (write: () => Promise<void>): Promise<boolean> => {
    setBusy(true);
    setProblem(null);
    try {
      await write();
      return true;
    } catch (error) {
      setProblem(errorMessageOf(error));
      return false;
    } finally {
      setBusy(false);
    }
  };
  return { busy, problem, setProblem, run };
}
