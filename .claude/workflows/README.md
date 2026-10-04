# Workflows

`nail-wave.js` runs one implementation wave: contract → parallel packages in
disjoint directories → integration → two reviews → fix → independent verify.
Every milestone since M2 has used it.

Agents work in the work folders, never in the master
(ADR 0132, docs/dev-setup.md "Work folders"): the lead stages (contract,
integration, reviews, fix, verify) in `nail-work-1` on `work-1`, parallel
package *i* in `nail-work-<N>` on `work-<N>`. Each package resets its folder
to the contract (`origin/work-1`), commits and pushes its branch; the
integrator merges the branches into `work-1`. After the verify stage passed,
the lead fast-forwards the master: `git -C <master> merge --ff-only work-1`.

## Starting a wave

1. Make sure the work folders exist (`node scripts/work-folders.mjs create <n>`
   from `nail-work-1`) and are clean.
2. Detail the wave in `docs/plan.md` first (work packages, scenarios, gate).
3. Write its arguments as `args/<wave>.json` (see `args/m5d.json` for the
   shape) and set `since` to the commit the wave starts from.
4. Ask Claude to run the `nail-wave` workflow with those arguments
   (`Workflow({ scriptPath: ".claude/workflows/nail-wave.js", args: … })`).

## Arguments

| Key | Meaning |
| --- | --- |
| `wave`, `key` | Display name and short key (target-dir suffixes) |
| `since` | Commit the verify stage lists changes from |
| `memoryName` | Memory file the integrator writes (`.claude/memory/<name>.md`) |
| `planRef` | Where in `docs/plan.md` the wave is described |
| `contract` | Task for the sequential contract stage |
| `slots` | Ownership map the contract prepares for the parallel packages |
| `build[]` | Parallel packages: `key`, `dirs` (owned paths), `npm` (may add npm packages), `prompt`, optional `folder` (work folder number; default lead + 1 + index) |
| `lead` | Work folder number of the lead stages (default 1) |
| `integrate` | Task for the integration stage |
| `reviews[]` | Review lenses (`key`, `lens`); one should always be quality or security |
| `verify` | Extra checks for the independent verify stage |
| `acceptContract` | `true` to continue past a contract stage whose only red results are expected (pre-existing lint findings, a known flake) |

## Resuming

A run can be resumed only from the same Claude session (`resumeFromRunId`).
From a new session, rerun the wave with the same arguments: finished stages
have already committed their work, so tell the contract stage (or skip it with
`acceptContract`) what is done, and check `git log` and the wave's memory file
first.
