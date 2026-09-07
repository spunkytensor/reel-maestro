# Reel Maestro Studio — design guide for AI agents

This guide tells a coding agent how to build Studio screens that match the approved mockups in
`docs/design/*.png`. It is normative: an implementation that follows this document and the token
file at `docs/design/src/studio.css` is correct; one that improvises is not. Where the visual notes
in `STUDIO_PLAN.md` §3 and this guide differ, this guide wins. The plan still owns behaviour,
safety and the CLI contract.

Use it in three ways:

1. Before writing a screen, read §1–§5 and the screen's entry in §7.
2. While writing, copy recipes from §3 and §6 instead of inventing values; use the vocabulary in §8.
3. Before finishing, run the checklist in §11 and render the screen the way §12 describes.

## 1. Design intent (quote this verbatim in prompts)

You are designing a video creation web app. The visual language is inspired by Apple Liquid Glass:
translucent layered surfaces that refract what is behind them, soft specular highlights along edges,
and content that feels like it floats above the canvas. The product is calm, precise and confident.
Nothing shouts. The user's generated video is always the most vivid thing on screen; the interface
is the frame, not the picture.

Two rules govern every screen:

- **Progressive disclosure.** The first layer of any surface shows only what most users need to
  move forward. Every advanced control lives one level deeper, behind an affordance labelled
  "Customize" (or "Advanced") that opens in place as a popover or sheet. Never expose more than one
  advanced concept at the top level.
- **One clear next action.** Every screen has exactly one primary button. It names the outcome
  ("Generate video", "Apply 3 changes", "Export MP4", "Download MP4"), never "Submit" or "Continue".

## 2. What Studio is, in the user's words

Studio turns a topic, a link or a script into a narrated, captioned video, then lets the user change
scenes and apply only the necessary work. The user sees **videos** (not runs), **versions** (not
revisions), **scenes** with **words**, an **image** and **motion**, **narration**, **music** and
**captions**. They never see providers, models, containers, queues, manifests, hashes, plans,
stages or file layouts unless they open Settings or a Customize layer. §8 maps every internal
concept to its user-facing word.

## 3. Materials

Describe surfaces as materials, not colours. Four materials, no others. Tokens live in
`docs/design/src/studio.css`; reuse them as CSS variables, never as literals.

| Material | Use | Light | Dark |
| --- | --- | --- | --- |
| Canvas | Page background, preview backing | `#F5F6F8` | `#0C0E12` |
| Glass | Panels, bars, side panel, timeline, settings groups | `rgba(255,255,255,.55)`, blur 24 px, saturate 160 % | `rgba(255,255,255,.08)`, same blur |
| Glass elevated | Popovers, sheets, success card | `rgba(255,255,255,.76)`, blur 40 px, shadow `0 20px 60px rgba(0,0,0,.16)` | `rgba(26,29,36,.70)`, blur 40 px, shadow `… rgba(0,0,0,.35)` |
| Ink | Text, icons, hairlines, fills | `#16181D` at 92 / 60 / 38 % (`--ink-1/2/3`) | `#F2F3F5` at 92 / 60 / 38 % |

Supporting tokens: `--hair` (ink 10 %) for row separators inside a panel; `--fill` (ink 6 %) and
`--fill-2` (ink 10 %) for neutral control backgrounds; `--field` for text fields
(`rgba(255,255,255,.72)` light, `rgba(255,255,255,.08)` dark); `--on-glass-pill` for the selected
segment and status pills.

Glass recipe (already implemented as `.glass` / `.glass.elevated`):

- `backdrop-filter: blur(24px) saturate(160%)`; elevated uses 40 px.
- Inner highlight `inset 0 1px 0 rgba(255,255,255,.35)`.
- Edge hairline drawn as a 1 px gradient border, brighter at the top (`--edge-top`) fading to the
  bottom (`--edge-bot`), via a masked `::before`.
- A faint diagonal specular sweep (`::after`, 3–7 % white) on large surfaces only.
- Radius 24 px for panels and popovers, 30 px (capsule) for the top bar, 20 px for tiles.

Behind the glass, the canvas carries a slow, soft colour field (three blurred radial gradients,
`--blob-1/2/3`) so refraction has something to catch. It is decoration for the glass, not for the
content; keep alphas at or below the token values.

Rules:

- Never stack glass more than two deep (panel → popover is the maximum).
- Never put text on blurred footage without a glass elevated panel under it.
- Test every elevated panel over the busiest frame of the preview; the dark elevated alpha is
  deliberately opaque for this reason. Do not lower it.
- No `rgba(0,0,0,.1)` card shadows, no borders on everything, no glow, no coloured drop shadows.
- Reduced transparency (`html[data-solid="1"]`, also when the OS asks): glass becomes opaque
  (`#FFFFFF` / `#181B22`, elevated `#1E222B`) and blur is removed. Layout must not change.

## 4. Accent

One hue only, used for the primary action and nothing else in the same view.

| Token | Light | Dark | Notes |
| --- | --- | --- | --- |
| `--accent` | `#F26B21` | `#FF7A33` | Reel Maestro orange; the brand's single accent |
| `--accent-2` | `#FFA45E` | `#FFB176` | Gradient end for the primary button |
| `--on-accent` | `#16181D` | `#16181D` | Text on accent; 5.6:1 light, 7:1 dark (AA). White on this orange fails AA — never use it |

The primary button is `linear-gradient(135deg, var(--accent-2), var(--accent))` with an inner top
highlight. Progress inside the button is a translucent white fill (`.btn.primary .fill`) growing
from the left, with a tabular time estimate in the label.

Everything else that needs emphasis uses Ink: selected segments are white/14 % pills, the active
toggle is `--ink-1`, edited-scene dots are `--ink-1`, selected timeline clips get an ink outline.
The keyboard focus ring is the one exception: `outline: 2px solid var(--accent); outline-offset: 2px`
on every interactive element, only while focused.

Status colours (`--ok`, `--warn`, `--err`) appear only as the 7 px dot inside a status pill and are
always paired with words ("Connected", "Needs attention"). They are not accents and never colour
text, backgrounds or icons.

Switching the accent (for example to the guide's violet-blue) means changing `--accent`,
`--accent-2` and re-checking `--on-accent` contrast. Nothing else references the hue.

## 5. Typography and layout

Two families: **Inter** (falls back to the system UI stack; SF Pro on Apple platforms) for display
and UI, **JetBrains Mono** (tabular) for timecodes, durations, costs and file sizes only. Web builds
load Inter and JetBrains Mono under the SIL Open Font License; the CLI and captions are unaffected.

| Role | Class | Size | Line | Tracking | Weight |
| --- | --- | --- | --- | --- | --- |
| Hero | `.hero` | 3.5 rem | 1.05 | −0.03 em | 600 |
| Title | `.title` | 2 rem | 1.15 | −0.02 em | 600 |
| Section | `.section` | 1.25 rem | 1.3 | −0.01 em | 600 |
| Body | `.body` | 1 rem | 1.55 | 0 | 400 |
| Label | `.label` | 0.875 rem | 1.4 | 0 | 500 |
| Caption | `.caption` | 0.75 rem | 1.4 | +0.01 em | 400, `--ink-2` |
| Timecode | `.tc` | 0.8125 rem mono | — | 0 | 400, tabular |

Rules: sentence case everywhere, including buttons and segment labels. No all caps, no eyebrow
labels, no single accented word inside a headline, at most two weights per screen (600 and
400/500). Body lines under 70 characters. Tertiary ink (38 %) fails AA and is for decoration and
disabled states only; real captions use `--ink-2`.

Layout: 8 px grid; component padding 16 / 20 / 24; section gaps 32 / 48 / 64. Glass panels float
with 16 px of canvas around them and never touch the viewport edge. Max content width 1280 px for
Start, Projects and Settings; the editor uses the full viewport because the preview is the anchor.
Content is left-aligned inside panels; centre alignment is reserved for the Start card and the
Exported moment.

Editor grid (desktop ≥ 1024 px): top bar 60 px capsule; below it a 320 px glass side panel and a
stage column; the stage holds the 16:9 preview (radius 24, on canvas, no glass) and a glass
timeline (≥ 120 px) with transport, clip track and chapter markers.

Responsive: at ≤ 1024 px the side panel collapses behind a "Scenes" button that opens it as a sheet;
at ≤ 600 px panels become full-width bottom sheets, the timeline collapses to a scrubber, the
primary action pins to the bottom with 12 px margins, and a vertical video is centred at a fixed
height with canvas either side. Everything must work down to 360 px wide.

## 6. Components

Sizes are in `studio.css`; the anatomy and rules are here.

- **Top bar** `.bar.glass` — 60 px, capsule, fixed 16 px from the edges. Left: brand mark and
  wordmark on Start/Projects/Settings, or back button + project name + "Version n · Saved" in the
  editor. Right: quiet actions, an in-progress pill when work is running, then the single primary.
  "by Spunky Tensor" appears as a caption beside the wordmark; the mascot illustration is not part
  of the chrome (About only).
- **Buttons** `.btn` — 40 px capsule, 500 weight, `--fill` background; `.quiet` transparent with
  `--ink-2` text; `.primary` accent gradient; `.sm` 34 px, `.lg` 48 px; `.icon` square. Minimum
  touch target 44 px (add padding on mobile).
- **Segmented control** `.seg` — capsule on `--fill`, selected item is an on-glass pill with a
  soft shadow. Use for 2–4 named choices. Full-width `.tabs` variant in panels.
- **Toggle** `.toggle` — 44 × 26, ink when on.
- **Field** `.field` — radius 16, `--field` background, inner top highlight, ≥ 48 px. `.big` for
  the Start prompt. Shows the focus ring only while focused; edited state is expressed in the
  caption below, not by colour.
- **Status pill** `.pill` — 28 px, on-glass background, optional 7 px dot in a status colour,
  always with words. Used on tiles and in Settings.
- **List and item** `.list > .item` — 52 px rows separated by `--hair`; label 0.9375 rem 500 with
  a caption below; control on the right. This is the Layer 2 building block (Customize popover,
  Settings, Export options).
- **Tile** `.tile` — 16:9, radius 20, no shadow; vertical posters sit centred on a blurred copy of
  themselves. Title and one caption line sit below the tile as plain text, never inside a card.
- **Scene row** `.scene` — 44 px: 52 × 30 thumbnail, "Scene n", first words as caption, an ink dot
  when the scene has unapplied changes. Selected row is an on-glass pill.
- **Timeline** `.timeline.glass` — transport row (play, tabular time, quiet shortcut hint,
  captions/volume/fullscreen), a clip track of equal-width thumbnails with numbers, an ink playhead,
  and chapter markers as captions.
- **Popover** `.popover.glass.elevated` — 400–420 px, radius 24, anchored beside the control that
  opened it, header row with a quiet close button, contents as a `.list`. No buttons other than
  quiet ones; its edits become unapplied changes.
- **Sheet** `.sheet.glass.elevated` — 440 px, docked right on desktop, full-width from the bottom on
  mobile; keeps the preview visible; footer with a quiet "Cancel" and the primary.
- **Success card** `.exported` — glass elevated over a dimmed editor: poster, "Exported", mono
  details, share link with "Copy link", quiet companion downloads, primary "Download MP4".

## 7. Screens, layer by layer

Each entry lists Layer 1 (visible on load), Layer 2 (behind Customize or a disclosure), the single
primary, and the states the implementation must render. File names refer to `docs/design/`.

### Start — `01`, `02`, `03`
- Layer 1: title "Describe the video you want", one large prompt field (topic, pasted link or
  dropped script — one field, three inputs), format segment (Widescreen / Vertical), length segment
  (widescreen only), quality segment (Quick draft / Balanced / Best quality), a caption summarising
  Layer 2 ("Narration, captions and music on · video clips off · Customize"), and the estimate
  caption ("Estimated cost $0.16 · ready in about 4 minutes").
- Layer 2 (Customize popover, opens beside the card): Narration (toggle; voice and pace),
  Music (Generated / None / Your track), Video clips (Off / Some scenes / All scenes), Captions
  (Burst / Karaoke / Boxed / Minimal), Spending limit, Models ("Balanced preset · Change").
- Primary: "Generate video". Progress shows inside it ("Generating video · 0:42") and the page
  moves to the editor as soon as a preview exists.
- States: empty, typing, estimate over the spending limit (caption explains, primary disabled),
  provider not connected (caption with a quiet "Connect" that opens Settings).

### Projects — `04`, `05`
- Layer 1: title and count caption, format filter segment, search in the bar, 3-column poster
  tiles with title + one caption line. Status pills on tiles only when something is happening:
  "Generating · 4:31", "Waiting", "Imported", "Needs attention".
- Primary: "New video" in the bar.
- States: empty (send the user to Start — there is no dashboard on first run), generating,
  waiting, imported (read-only until the user opens it), needs attention, search with no results.

### Editor — `06`, `07`
- Layer 1: bar (back, name, version, undo/redo, "Versions"), side panel tabs Scenes / Narration /
  Music with the scene list, preview, timeline. Side panel footer: a caption and "Customize scene".
- Layer 2: the scene popover (`08`), narration and music tabs.
- Primary: "Export MP4" when nothing is pending; "Apply n changes" when there are unapplied
  changes; the same button shows "Applying changes · m:ss" with an inner progress fill while work
  runs (`10`). Never two primaries.
- States: clean, with unapplied changes (ink dots on scenes, caption lists what changed),
  applying (side panel rows show what is being regenerated), interrupted (in-progress popover offers
  "Resume"), version switching.

### Scene customize — `08`
- Popover beside the selected scene: Words (caption: "Changing the words re-records the whole
  narration."), Image, Motion, and "Current image · From version 1" with Keep / Regenerate.
- No primary. Edits accumulate as unapplied changes on the editor's primary.

### Review changes — `09`
- Sheet docked right, preview still visible. Title "Apply n changes"; caption "About $0.01 and
  6 minutes. Version 3 stays exactly as it is." Groups: Regenerate, Keep, Remove, Restyle — one
  short line per item, icon in `--ink-2`.
- Primary: "Apply changes"; quiet "Cancel". Approval is bound to the plan hash server-side; the hash
  is never shown. If inputs change before approval, the sheet re-opens with the new plan.

### In progress — `10`
- Bar shows a quiet pill "n in progress"; its popover lists jobs with a caption status, tabular
  elapsed time, a thin ink progress bar, quiet "Cancel", and "Resume" for interrupted work.
  Footer caption: "Nothing restarts on its own. You can close this and keep editing."
- Cancelling is quiet, never destructive-red. Remote work that may still finish is described in
  words ("Two clips were already requested and may still arrive.").

### Export — `11`, `12`
- Side panel becomes the Export panel: preset segment (Web / Social / Pro), a quiet technical
  caption ("1080p, H.264, 12 Mbps, stereo · Customize"), toggles for Captions, Poster as cover,
  Watermark (off by default; branding never adds one). Footer caption with destination and time.
- Primary: "Export MP4" → "Exporting · 1:10" inside the button → the Exported card with share link,
  companion downloads (Poster, Captions, Description and chapters) and primary "Download MP4".

### Settings — `13`, `14`
- Glass groups of `.item` rows: Appearance (Theme System/Light/Dark, Reduce transparency, Reduce
  motion), Generation (AI provider with "Connected" pill and quiet "Change key", default format,
  default quality, spending limit), Videos (saved location and size, rescan, export watermark, keep
  earlier versions), About (version, "by Spunky Tensor", Licenses, Help).
- No primary. Keys are write-only; never show a saved value.

### Mobile — `15`, `16`, `17`
- Start card is full-width with the primary pinned at the bottom. Editor shows a centred vertical
  preview, a glass scrubber with tabular time and clip thumbnails, and a bottom sheet with the scene
  tabs and list; the primary is pinned. Exported is a centred card with the primary pinned.

## 8. Vocabulary — internal concept to user words

Never leak the left column into the UI. Layer 2 may show model names under "Models"; nothing else.

| Internal / CLI | UI |
| --- | --- |
| run, output folder | video, project; "saved on this computer" |
| revision, `revisions/rev-n/` | version, "Version 3" |
| `--format reel` / `youtube` | Vertical / Widescreen |
| `--quality draft` / `standard` / `premium` | Quick draft / Balanced / Best quality |
| `--minutes` | "About 1 minute / 3 minutes / 6 minutes" |
| stills, `scene-NN.jpg` | images (a scene's image) |
| clips, `scene-NN.mp4`, `--video`, `--video-scenes` | video clips: Off / Some scenes / All scenes |
| video provider, local H3, OpenRouter, model ids | not shown; Settings → "AI provider · Connected"; Layer 2 → "Models" |
| `--music-gen`, uploaded music, `--mix` | Music: Generated / None / Your track; "Ducks under the narration" |
| TTS, voice, `--speed` | Narration; "Automatic voice · normal pace" |
| Whisper, `words.json`, timing | not shown ("narration and timing" only in the review sheet) |
| `--caption-style` presets | Captions: Burst / Karaoke / Boxed / Minimal |
| `--max-cost`, estimate guard | Spending limit; "Stops before starting if the estimate is higher" |
| cost estimate with range | "About $0.01 and 6 minutes" (range in a caption if needed) |
| plan: reuse / generate / derive locally / exclude | Keep / Regenerate / Restyle (or "Update") / Remove |
| approval hash, idempotency key | not shown |
| job queued / running / succeeded / with warnings / failed / interrupted / cancelled | Waiting / Generating (or Applying changes, Exporting) / Ready / Ready, with a note / Needs attention / Interrupted / Stopped |
| legacy folder, no manifest | Imported; "from an earlier version of Reel Maestro" |
| container, Docker, bind mount, `/data/out` | not shown; "saved on this computer", "./out" only in Settings |
| SSE reconnect, event replay | invisible; state simply stays correct |
| poster | Poster (as cover) |
| `youtube.md` metadata | Description and chapters |

Copy rules: plain verbs, sentence case, no filler, no arrows appended to links or buttons, no
"Save" anywhere (everything autosaves; "Saved" is a caption). An action keeps its name through its
lifecycle: Export → Exporting → Exported; Apply changes → Applying changes → Applied. Errors say
what happened and what to do next: "Generation stopped because the spending limit is $5.00 and this
video is estimated at $6.20. Raise the limit or choose Quick draft." Warnings from the pipeline are
rewritten into user terms ("Scene timing in chapter 2 is approximate.").

## 9. Progressive disclosure and flow

Design every feature in two layers. Layer 1 is the 20 % of controls that cover 80 % of use:
presets, one segment, one toggle, one short list. Layer 2 opens from "Customize" in place, remembers
whether it was open per user, and never opens by default. Show the current Layer 2 state as a quiet
caption under the Layer 1 control so power users see it without opening anything.

| Feature | Layer 1 | Layer 2 |
| --- | --- | --- |
| Generate | Prompt, format, length, quality, Generate video | Narration, music, video clips, captions, spending limit, models |
| Scene | Select in list, preview | Words, image, motion, keep or regenerate |
| Apply | "Apply n changes" | Review sheet with keep / regenerate / remove / restyle and cost |
| Export | Preset, Export MP4 | Captions, poster, watermark; codec and bitrate in Customize |
| Progress | Progress inside the button | In-progress popover with cancel and resume |

The core path is Start → editor preview → refine → export, completable in under two minutes on
first use. Never block on a modal; use sheets and popovers that keep the preview visible. Long
operations show progress inside the control that started them with a tabular time estimate. The
back path is always visible and cheap: undo is global, versions are automatic and immutable.

## 10. Motion and accessibility

- Motion only in response to a user action. Page load: one soft settle of the main glass panel
  (300 ms ease-out) at most. Popovers and sheets: scale 0.96 → 1 with opacity, 220 ms,
  `cubic-bezier(0.2, 0.8, 0.2, 1)`. Hover on glass brightens the edge hairline by 10 %; no lift,
  no shadow change. Scrubbing and dragging are direct with no easing.
- `prefers-reduced-motion`: replace every transition with an instant state change.
- `prefers-reduced-transparency` or the Settings toggle: solid materials (§3).
- WCAG 2.2 AA for all text on glass, tested over the busiest frame. Focus ring on every
  interactive element (§4). Touch targets ≥ 44 px. Statuses never rely on colour alone.
- Keyboard: arrow keys move between scenes, modifier + arrows reorder, Delete removes (with undo),
  Space plays; the shortcut hint in the timeline stays a caption.
- Live announcements for progress are throttled (stage changes, not every tick) and phrased in
  user words.

## 11. Anti-patterns — reject any output that contains these

- More than one accent-coloured element in a view (focus ring excepted, and only while focused).
- Card grids where every card has the same radius and shadow; cards with borders and gray shadows.
- Glass on text-heavy areas without the elevated material; text on blurred footage without a panel.
- Advanced settings visible on first load; a gear icon standing in for the word "Customize".
- Model, provider, container, queue, manifest, hash, stage or file-path language outside Settings
  and the Models row.
- All-caps labels, eyebrow labels, arrow characters appended to links or buttons, a "Save" button,
  numbered step markers on content that is not a sequence.
- A modal that hides the preview; a second primary button; a red destructive Cancel.
- Invented percentages for whole-job progress; show stage-based progress and elapsed time instead.

## 12. Verifying a screen

1. Render both themes at 1440 × 900 and 390 × 844, plus the reduced-transparency variant, with
   the same real posters and stills used in the mockups. Compare side by side with the matching
   PNG in `docs/design/`.
2. Count accent elements: exactly one (or zero, in Settings and while a sheet holds the primary).
3. Check every text style against the contrast table over the busiest preview frame.
4. Tab through the screen: focus rings visible, order sensible, every popover and sheet closes with
   Escape and returns focus.
5. Read every string aloud against §8; anything from the left column is a defect.
6. Keep the screenshots with the implementation results, as `STUDIO_PLAN.md` §8 requires.

Mockup sources: `docs/design/src/*.html` (states via query flags, e.g. `?theme=dark&drafts=1&sheet=review`),
tokens in `docs/design/src/studio.css`, renderer `node docs/design/src/render.js`.

## 13. Prompt template

Use this when asking an agent for a screen or component:

```text
Design and implement the [screen or component] for Reel Maestro Studio.
Follow docs/design/DESIGN_GUIDE.md: four materials (Canvas, Glass, Glass elevated, Ink)
from docs/design/src/studio.css, one accent element per view, Inter plus a tabular mono,
sentence case, 8 px grid, the video preview as the visual anchor.
Layer 1 shows only: [list]. Layer 2, behind "Customize" and opened in place, contains: [list].
The single primary action is "[verb + object]"; progress shows inside it.
Use only the user vocabulary in guide §8; never mention providers, models, containers or files.
Motion only in response to user action; honour reduced motion and reduced transparency.
Output: [React + TypeScript component with CSS variables / HTML mockup], no hard-coded colours.
Do not use: all-caps labels, gray card shadows, gradient decoration, more than one accent
element, modals that hide the preview, a Save button, arrows appended to links.
Match docs/design/[NN-name].png and render both themes at 1440×900 and 390×844 to verify.
```
