# ADR 0143: P1 WP193 — tests that were flaky under machine load

Status: accepted · 2026-10-05 · P1 WP193 (stability; integrated by ADR 0144)

## Context

M5's verify and the P1 contract saw Vitest files fail only when the machine was
busy (cargo builds, a second test run): 20 s timeouts in layout-editor, pim
(`a11y`, `calendarList`), rules, security (`m4c`, `KeyManager`), palette and
plug-in extension tests; React's "not wrapped in act(...)" warning from the
customization editors, the palette and the rule manager;
`ui/build/strictBuild.test.ts` "fails on Vite's chunk-size warning" building
successfully; and the visual screens High contrast · Comfortable `html` /
`main-1024` with a different message-list scroll position. Earlier fixes had
raised single test timeouts to 60 s and 180 s. WP193 had to find the causes
instead.

## Decisions

### Cause 1: jsdom matched every theme rule against every element

A CPU profile of the slowest test (`RuleManager` "creates a rule with nested
conditions and actions", 10.8 s alone) showed 55 % of the time in jsdom's
`getComputedStyle`. The compiled theme puts about 2,370 style rules into the
test document, nearly all keyed on `[data-part="…"]`. jsdom's pre-filter only
knows ids, classes and tag names, so it runs the selector engine for every rule
on every element (~30 ms per element), again for each ancestor of an inherited
property, and again after every DOM change. Testing Library's role queries
(accessible names, `isInaccessible`), user-event's pointer-events check and axe
call it for nearly every element.

`ui/src/test/fastStyles.ts` (installed by `setup.ts`) puts a pre-filter in
front of jsdom's selector check: a rule whose selector requires
`[data-part="x"]` on its subject in every branch of its list cannot match an
element with another (or no) `data-part`, so it is rejected without the
selector engine; all other rules (34 of 2,370: `*`, `:root`, `… > *`, …) go
through jsdom unchanged. The computed styles stay exactly the same:
`fastStyles.test.tsx` compares them with and without the filter on rendered
widgets with the real theme (and fails when the filter is broken on purpose),
and fails when a jsdom update changes the internals the filter uses
(`Document#_getDOMSelector`, through `jsdom/lib/generated/idl/utils.js`), so
the suites never silently get slow again. Rejected alternatives: a smaller test
theme (the tests would stop exercising the real one), turning off user-event's
pointer-events check or role queries' hidden checks (they test real behaviour),
higher timeouts (the task forbids them as the fix).

### Cause 2: axe built a CSS selector for every passing node

After the first fix, the PIM event editor's axe scan still took 7 s: axe's
reporter generated selectors for every node of every *passing* rule
(`createReport` → `getSelector`). `ui/src/test/axe.ts` now asks for
`resultTypes: ["violations"]`; violations are reported exactly as before. The
palette's own `axe.run` call uses the shared helper (which also runs inside
`act`).

### Cause 3: late answers after the last assertion (act warnings)

Many widget tests end right after their last `findBy…`, while the app still
works (a save invalidates a list that refetches, a mutation settles). On an
idle machine `setup.ts` unmounted first; under load the answer landed first,
outside `act`, and the console check failed the test. `createEnv` now
registers each env (`ui/src/test/liveEnvs.ts`), and `setup.ts`'s `afterEach`
waits (at most 3 s, never failing; nothing under fake timers) until those envs'
queries and mutations are idle and flushes React inside `act` before
unmounting. Tested in `liveEnvs.test.tsx`.

### Cause 4: a lazy editor scanned in its "Loading…" state

`pim/a11y.test.tsx` waited only for idle queries: a lazily loaded editor first
shows its Suspense fallback (React reveals the content up to ~300 ms later), so
the dialog scans checked "Loading…" and the date-picker test found the editor
only when an earlier test had loaded it (it failed when run alone). `settle`
now also waits until nothing is `aria-busy`. Its 60 s / 180 s timeouts are gone.

### Cause 5: user-event waited a macrotask per keystroke

With its default `delay: 0`, user-event waits for a timer between every key
and pointer action. Typing a 70-character recovery key in `security/m4c`
took 1 s alone and up to 15 s with two other test runs on the machine. The
WP193 test files (security m4c / KeyManager, rules, palette, pim, the
customization editors, plug-in extension a11y) use
`userEvent.setup({ delay: null })`: the actions run back to back, each still
inside Testing Library's `act` wrapper, so React commits after every event
as before.

### Cause 6: the chunk-size warning arrives asynchronously

Vite 8 reports chunks over `build.chunkSizeWarningLimit` from Rolldown's native
reporter; its log reaches Vite's logger through an asynchronous callback, on a
busy machine after `closeBundle`, where `strictBuild` checks. The
`nail:strict-build` plugin now measures the chunks itself in `generateBundle`
with the reporter's rule (minified client builds, not libraries; bytes / 1000
over the limit) and records the warning deterministically
(`largeChunks`, tested; the app's own build still passes).

### Cause 7: where the virtualizer stopped scrolling

Selecting a message lets TanStack Virtual scroll the list to keep the row
visible, first with estimated row heights, then corrected after measuring:
where it stopped depended on timing. The visual harness (`screens.ts`
`pinListScroll`) waits until the list stopped scrolling, then sets the position
the virtualizer aims for (the row's end at the bottom edge, or the top when the
row is visible from there) and repeats until it holds. All 230 baselines still
match; none changed.

## Results

- The 12 files named in the plan, alone: 5 m 18 s before, 48 s after.
- The affected suites (a11y, customization, plugins, security, pim, palette,
  rules, layout-editor; 651 tests), two runs at once: about 300 s per run and
  one 20 s timeout (`RuleManager`) before; three runs at once after: about
  75 s per run, no failure.
- The whole UI suite (2,298 tests): about 65 s.
- Proof: 5 rounds of two parallel Vitest runs of the affected suites plus
  `src/test`, `toolbar-editor` and `strictBuild` (708 tests each) and, at the
  same time, the visual screens High contrast · Comfortable `html` /
  `main-1024`: all green, 55–108 s per run, the slowest single test 9.6 s
  (the 20 s timeout is unchanged).

## Consequences

- Every jsdom suite gets the faster computed styles and the settle step; a test
  that leaves a request pending on purpose costs up to 3 s more.
- A jsdom upgrade that changes `_getDOMSelector` fails `fastStyles.test.tsx`;
  adapt the filter then (or drop it and expect slow suites).
- `src/a11y.test.tsx` (not WP193's file) still carries raised timeouts that are
  no longer needed.
