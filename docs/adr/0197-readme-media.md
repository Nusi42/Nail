# 0197 — README with recorded animations

Status: accepted (2026-10-17)

## Context

The repository goes to GitHub (ADR 0198), and its front page is the README.
Requirement: a README with GIFs (and possibly
animations) that says why the app is called Nail: it is the last nail in
Outlook's coffin. Hand-made screen recordings go stale with every UI change,
can show private data, and no GIF tool (ffmpeg, ImageMagick) is installed on
the development machine.

## Decision

- **Recorded, not hand-made.** `npm run readme:media`
  (`scripts/readme-media.mjs` → `e2e/readme/playwright.readme.config.ts`)
  plays scripted scenes (`e2e/readme/media.readme.ts`) against a fresh
  `nail serve --seed demo-multi` per scene, with the visual harness's fixed
  clock, and writes `docs/media/<scene>.gif` and the still `calendar.png`.
  The hero (`hero.gif`) is the main window morphing through Frost, Sage,
  Nocturne and Graphite: each theme holds about two seconds, then the next
  one wipes in diagonally (`e2e/readme/lib/morph.ts`; a wipe changes only a
  band per frame, so frame diffing keeps it small; High contrast stays in
  the theme scene only). Only the demo personas and made-up
  domains appear (the phishing scene uses the made-up `ńovabank.ċom`).
  Not part of `npm run e2e`; the files are reviewed before they are
  committed. `NAIL_README_FRAMES=<dir>` also writes every frame as a PNG.
- **A drawn pointer.** `e2e/readme/recorder.ts` overlays a mouse pointer
  (pointer-events: none) that glides to the next target in eased steps and
  shows a ripple where it clicks, so a viewer can follow the scene; it is
  part of the recording only.
- **GIF encoding with `gifenc`** (MIT, no dependencies, dev dependency of
  `e2e`): per-frame palettes, and pixels unchanged since the previous frame
  written as the transparent colour (`e2e/readme/lib/gif.ts`), which keeps a
  scene at about 200–550 KiB. Frames are decoded with the visual harness's
  PNG decoder; its test encoder moved to `e2e/visual/lib/png-fixture.ts`
  for both tests.
- **An animated banner** (`docs/media/banner.svg`): a hammer drives the
  last nail into a coffin with an old envelope carved into it, in CSS
  animations, still under `prefers-reduced-motion`. No product logos.
- **The README** uses HTML for the centred banner and the full-width
  images, so it disables markdownlint's MD033 / MD041 in that file only,
  with the reason in a comment.

## Consequences

- After visible UI changes, `npm run readme:media` refreshes the media; a
  scene that no longer finds its controls fails like a test.
- Recording the scenes found a real defect: the look-alike sender warning
  ran its two sentences together and showed its advice in the monospace
  style of link targets. The sentences are one text now, and the advice is
  the new part `security-warnings.hint`.
