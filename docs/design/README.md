# Reel Maestro Studio — design mockups

Static design mockups for the Studio described in [`STUDIO_PLAN.md`](../../STUDIO_PLAN.md).
They follow a Liquid Glass design guideline: four materials (Canvas, Glass, Glass Elevated, Ink),
one accent hue used once per view, Inter for UI and JetBrains Mono for numbers, sentence case,
8 px grid, progressive disclosure behind "Customize", and one verb-object primary action per screen.
No mockup substitutes for a working UI; these set the visual bar for delivery step 3.
The normative rules, tokens, vocabulary and checklists for implementing them are in
[`DESIGN_GUIDE.md`](DESIGN_GUIDE.md), written for AI coding agents.

| File | Screen |
| --- | --- |
| `01-start-light.png`, `02-start-dark.png` | Start: describe, paste a link or drop a script; format, length, quality; estimate; Generate video |
| `03-start-customize-dark.png` | Start with the Customize layer open in place (narration, music, video clips, captions, spending limit, models) |
| `04-projects-light.png`, `05-projects-dark.png` | Projects: poster tiles with status pills (generating, waiting, imported, needs attention) |
| `06-editor-light.png` | Editor: scene panel, video preview as the anchor, glass timeline with tabular timecode and chapter markers |
| `07-editor-dark-drafts.png` | Editor with unapplied changes; primary becomes "Apply 3 changes" |
| `08-scene-customize-dark.png` | Scene customize popover: words, image, motion, keep or regenerate |
| `09-review-changes-dark.png` | Review sheet before applying: regenerate / keep / remove / restyle, cost and time, preview stays visible |
| `10-applying-activity-light.png` | Progress inside the primary button; in-progress popover with cancel and resume |
| `11-export-light.png` | Export panel: preset, quiet technical summary, Export MP4 |
| `12-exported-dark.png` | Export success moment with share link and companion downloads |
| `13-settings-light.png`, `14-settings-dark-solid.png` | Settings; the dark render uses the reduced-transparency (solid) option |
| `15-mobile-start-light.png`, `16-mobile-editor-dark.png`, `17-mobile-exported-dark.png` | Mobile: bottom sheet, scrubber, pinned primary action |
| `18-materials.png` | Materials, accent, type scale, controls and glass-over-footage reference in both modes |

## Regenerating

```sh
cd docs/design/src
npm install            # playwright-core; uses the Chromium already cached by Playwright
node render.js         # writes ../NN-name.png; `node render.js editor` renders a subset
```

Sources: `src/*.html` (one file per screen; `?theme=dark`, `?solid=1` and variant flags such as
`?drafts=1&sheet=review` select states), `src/studio.css` (tokens and materials), `src/shell.js`
(icon sprite). Thumbnails in `src/assets/` are downscaled stills from local `out/` runs; Inter and
JetBrains Mono are bundled under the SIL Open Font License for previewing only — the app itself uses
the system UI font stack.
