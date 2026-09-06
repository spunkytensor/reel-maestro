# Reel Maestro Studio — implementation plan

Status: proposed; planning only. No frontend, server, Docker image, or new CLI behavior is
implemented by this document. Branch: `plan/reel-maestro-studio`. No PR is requested.

## Outcome and scope

Build a beautifully designed, browser-based **Reel Maestro Studio**, with Spunky Tensor
attribution, that can browse runs, submit generation jobs, track progress, preview media,
edit scenes, and regenerate only affected assets. Keep the existing Rust CLI fully usable.
Run the production frontend, backend, CLI, and local media tools inside Docker; bind-mount
host `./out` so every generated artifact remains inspectable without entering a container.

Assume a single-owner, local-first studio for the initial release. Multiple browser tabs and
queued jobs are supported; public hosting, collaborative editing, accounts, and a general-purpose
nonlinear video editor are not initial goals. Docker is the complete recommended Studio
installation, not a reason to remove native CLI installation.

## 1. Architecture recommendation

Use **React + TypeScript + Vite** for the frontend and **Node.js LTS + TypeScript + Fastify**
for a thin backend. Use accessible headless primitives (for example Radix), CSS design tokens,
and ordinary CSS for the visual system. Use SQLite for job state and event replay; no Redis,
separate database service, or distributed queue is necessary initially.

The backend serves the compiled frontend and API on one origin and runs the Rust executable
as a child process. Rust remains the sole authority for configuration resolution, generation,
cost planning, dependency invalidation, and artifact reuse. TypeScript owns HTTP, uploads,
the durable queue, child supervision, and browser sessions—not a second media pipeline.

```text
Browser: React / TypeScript / HTML / CSS
                  │ same-origin HTTP + server-sent events
                  ▼
Docker: Fastify + SQLite + single active job supervisor
                  │ fixed executable, argument array, structured events
                  ▼
        Rust reelmaestro → ffmpeg / ffprobe / Whisper
                  │                    │
                  ▼                    ▼
          /data/out bind mount    external provider APIs
                  │              OpenRouter / optional H3
                  ▼
              host ./out
```

**Rust alternative:** Axum can serve the same TypeScript frontend, SQLite queue, and SSE API.
It removes Node from the runtime and can share Rust types directly, but requires Rust web-server
work and still needs a JavaScript build toolchain. Recommend Fastify here to match the requested
conventional TypeScript backend and keep UI/API development straightforward. The versioned CLI
contract keeps switching to Axum possible without rewriting the frontend or generation pipeline.
Do not implement both backends. Preserve process isolation even if Axum is chosen.

Proposed ownership: `studio/web/`, `studio/server/`, existing Rust stage modules under `src/`,
and root `Dockerfile`, `compose.yaml`, `.dockerignore`. Introduce focused Rust planning/event
modules only where they own real shared behavior; do not rewrite all stages into a framework.

## 2. Verified current behavior and prerequisites

Source of truth: `src/main.rs` (`Cli`, orchestration and cost guards), `src/config.rs`,
`src/model.rs`, `src/images.rs`, `src/video.rs`, `src/video/local.rs`, `src/transcribe.rs`,
and `src/assemble.rs`.

- `--from` loads `script.json` and reuses `audio.mp3`; missing audio currently fails rather
  than regenerating narration. Existing `words.json` is reused without checking changed text.
- Stills and hosted clips reuse `scene-NN.jpg` / `scene-NN.mp4` by existence. Scene objects do
  not have stable IDs. Removing or reordering scenes can associate the wrong files with a scene.
- Local H3 has a job/fingerprint manifest and resumable accepted jobs. Preserve that behavior;
  reconcile it with the new shared dependency plan rather than discarding accepted jobs.
- `--video-scenes N` selects the first N scenes, not arbitrary scene IDs. Arbitrary scene
  upgrades need a backwards-compatible CLI extension.
- Resume keeps an existing soundtrack unless another is requested; `--music-gen` is an explicit
  generation request. A true remove-music action needs explicit semantics, not just omission.
- Assembly runs again on resume; an existing poster is reused. Still and video outputs can
  coexist as `reel.mp4` and `reel-video.mp4`, but those are not a complete revision history.
- Stored script format wins on resume. Switching aspect ratio cannot be presented as a cheap
  render-only toggle for assets created with different geometry.
- Progress is human-readable terminal output, including local-provider status/elapsed reporting;
  there is no public machine-readable pipeline event protocol. Optional stage failures may
  produce usable fallback output, so successful exit alone does not mean every request succeeded.

**Do not ship scene editing as “edit JSON and rerun.”** Safe dependency-aware resume is a
prerequisite for the promised regeneration experience, and must also benefit native CLI users.

## 3. Visual direction: refined Cupertino, recognizably Reel Maestro

Use Apple's Liquid Glass as visual inspiration, not copied assets or an Apple-branded clone.
The existing `logo.jpg` combines navy, orange, gold, a conductor, and film imagery. Preserve
that identity: restrained orange primary actions, deep navy/ink surfaces, generous whitespace,
and media-led composition. Keep the playful illustration in onboarding and empty states;
use a compact readable Reel Maestro identity in the navigation rather than shrinking the
entire detailed illustration into an unreadable icon.

The local `watermark_transparent_spunkytensor.png` depicts a lightning bolt and orbital rings.
It is currently untracked user material: do not modify or implicitly commit it. During asset
integration, confirm it is the intended distributable Spunky Tensor logo, verify alpha/rights,
and add an approved web asset with attribution. Show “by Spunky Tensor” in navigation/about,
with the mark on a subtle neutral backing where needed for contrast. App branding does not
silently enable a watermark on exported videos; export watermarking remains a user setting.

Design specification:

- Light: warm off-white canvas, white content cards, ink text, orange accents. Dark: near-black
  navy canvas, elevated charcoal surfaces, warm-white text, brighter orange accents.
- System / Light / Dark selector; initialize from OS preference, persist explicit preference,
  avoid a flash of the wrong theme, and react to OS changes in System mode.
- Liquid-glass treatment only on the sidebar, floating transport bar, and inspector chrome:
  restrained translucency, blur, fine highlight borders, and soft shadows. Text-heavy cards,
  forms, and media retain stable opaque backgrounds and contrast.
- System UI font stack (`-apple-system`, `BlinkMacSystemFont`, `Segoe UI`, sans-serif), not
  bundled proprietary Apple fonts. Clear hierarchy, tabular progress numbers, 8px spacing
  rhythm, approximately 16–24px panel radii, and comfortable touch targets.
- Short 150–220ms transitions; no bouncing decoration. Honor reduced motion and reduced
  transparency where available, with an explicit solid-surface option and no-blur fallback.
- WCAG 2.2 AA contrast, visible keyboard focus, labeled controls, non-color-only statuses,
  accessible dialogs, keyboard scene movement, and live announcements that do not spam.
- Desktop: sidebar + central preview/storyboard + inspector. Tablet: collapsible inspector.
  Mobile: single-column workspace with bottom navigation and sheet-based settings; preview
  retains the correct reel or landscape aspect ratio without horizontal page scrolling.

Screens and important states:

1. **Library:** poster-led grid/list, search, format/status filters, recent activity, new-video
   action, empty state, and damaged/legacy run indicators. Import existing host output folders
   automatically through a safe rescan; do not trust arbitrary filesystem paths from the browser.
2. **Create:** topic, brief, verbatim script, URL, or existing run; prominent format and quality
   controls, expandable advanced settings, estimated work/cost, and one clear submit action.
3. **Workspace:** playable video, scene filmstrip, narration/prompt inspector, per-scene
   still/video previews, chapter navigation, undoable draft changes, and revision comparison.
4. **Activity:** persistent queue, stage checklist, scene-level status, elapsed time, warnings,
   optional diagnostics, cancel, and explicit resume/retry after interruption.
5. **Export/settings:** final video/poster/captions/metadata download, artifact browser,
   provider health, installed fonts/Whisper models, defaults, and credential configuration status.

## 4. Feature-complete configuration

Expose every supported generation feature through typed controls. Generate a versioned settings
schema from Rust's CLI/config definitions, including constraints, defaults, choices, conditional
availability, and help text; derive TypeScript types and add a parity check so future flags do
not silently disappear from Studio. Rust validates resolved submissions again. Display effective
values and their source (request, environment, quality tier/default), excluding secrets.

| Panel | Current CLI coverage / behavior |
| --- | --- |
| Source | `topic`, `brief`, `script`, `url`, `from`; mutual exclusion and uploaded text |
| Format/budget | `format`, `minutes`, `quality`, `dry-run`, `max-cost` |
| Narration | `voice`, `tts-model`, `speed`, `no-narration`, `scene-seconds` |
| Script/imagery | `text-model`, `image-model`, `judge-model`, `validate-scene`, `no-consistency`, `character-ref` |
| Video | `video`, `video-scenes`, `video-provider`, `video-model`, `video-resolution`, `video-size`, `video-input-mode`, `video-seed`, `video-steps`, `video-wait-timeout` |
| Music | uploaded `music`, `music-gen`, `music-model`, `mix`, `music-volume`; add explicit keep/remove/regenerate actions |
| Captions/timing | `no-captions`, `caption-style`, `caption-font`, `whisper-model`, diagnostic `no-images` |
| Finish | `no-dissolve`, `dissolve-seconds`, `no-grade`, `no-loudnorm`, `poster-scene`, `no-embed-poster`, `watermark` |
| Diagnostics/runtime | `verbose`, `out`, `whisper-cmd`, `video-base-url`, provider credentials |

Runtime controls are intentionally constrained: `out` stays inside the mounted root, uploads
become backend-owned asset IDs, `whisper-cmd` chooses only installed allowlisted executables,
and provider URLs are operator-configured/allowlisted. Support configuration of these features,
not browser-supplied shell commands or arbitrary server paths. Secrets may be provisioned through
Docker secrets or write-only settings with protected server storage; never return saved values.

Quick Preview means a clearly described still-based draft with video/music generation off,
unless explicitly enabled. It still incurs script/TTS/image/poster costs. `--no-images` is a
timing diagnostic, not a playable preview. Upgrading the quality preset must show which models
change and offer keeping existing assets versus explicitly regenerating affected assets; it
must not silently turn an inexpensive video upgrade into a full regeneration.

## 5. Safe revisions and selective regeneration

Add a versioned Rust run manifest with stable scene IDs, resolved non-secret configuration,
artifact content hashes, dependency fingerprints, provider job references, and completion
states. Use deterministic cryptographic hashes, not unstable process/platform hash functions.
Keep human-inspectable filenames and backwards-compatible script loading.

Studio edits create immutable completed revisions and an isolated writable draft/job directory,
for example `out/<run>/revisions/<revision>/`. Each executable revision contains a normal CLI
run layout and can be resumed with `--from`. Reuse validated assets by reflink or copy initially;
do not hard-link files that tools may overwrite. Store parent/revision provenance. Publish files
via temporary file + atomic rename, and publish completed revisions only after output validation.
Lock a revision against concurrent workers, including native CLI invocations using the new
manifest. Detect host-side edits through hashes; reject/replan stale approvals rather than race.

Extend `--from` to accept an explicit, validated edit/regeneration plan. Exact flag names are
to be settled during CLI implementation; this document does not describe existing switches.
Plans distinguish **reuse**, **generate**, **derive locally**, and **exclude** for each artifact,
with reasons. Exclusion from a revision does not delete prior artifacts. Maintain a separate
“keep existing asset” choice when intentionally retaining media generated under older settings;
preserve its true provenance instead of relabeling it as generated with new settings.

| Change | Required work; what stays reusable |
| --- | --- |
| Caption style/font, watermark, grade, transitions, mix/gain, loudness | Local caption/render/mux work only; retain narration, timings, stills, clips, music, and poster unless their own inputs change |
| Add video to selected stable scene IDs | Generate only missing/explicitly invalidated selected clips; reuse script, audio, timings, stills, soundtrack, poster; render new video variant |
| Add/change generated music | Generate soundtrack only when requested; reuse all unrelated assets, then remix/render |
| Remove music or switch video back to stills | Exclude assets from this revision and locally render; retain source files and earlier versions |
| Change one image prompt | Regenerate that still and dependent first-frame clip; text-only clips depend on their actual prompt inputs, not an unused still |
| Change motion prompt/provider/seed/steps/clip geometry | Regenerate affected selected clips only, unless the user explicitly chooses to retain them |
| Change entity/reference | Regenerate only assets whose recorded generation inputs used that entity/reference, including poster when applicable |
| Delete a visual scene, preserve speech | Require reassignment of its narration to remaining scenes; validate complete coverage, recalculate cuts, reuse narration/timings and surviving media by ID |
| Delete scene and its spoken text, reorder speech, or edit narration/voice/speed | Rebuild affected narration and timings, then dependent cuts/captions/render; retain unchanged visuals; regenerate clips only if actual generation inputs/duration requirements change |
| Silent timeline duration change | Rebuild silent track/timeline and dependent local outputs; preserve unrelated generated imagery |
| Change poster concept | Regenerate poster and re-embed if enabled, without regenerating scenes/audio |
| Change aspect ratio | Fork an explicit new-format revision; regenerate geometry-dependent assets with approval, reuse audio/music if still valid |

Because current narration is usually one synthesized track, text deletion may require regenerating
that entire track. Do not promise free edits or split TTS into per-scene calls that change voices.
For existing per-chapter fallback tracks, reuse only genuinely independent unchanged chapters.
Scene edits must also update chapter ranges, chapter narration, total narration, and timestamps;
reject invalid or empty timelines before generation. Script lines must cover the intended narration.

Legacy folders have unknown provenance: import read-only first, assign stable IDs in a revision,
validate media, and present explicit trust/reuse choices. Never fabricate historical fingerprints.
Preserve the original folder and require confirmation before paid replacement of unknown assets.

## 6. CLI contract, progress, and durable jobs

Add opt-in versioned JSON configuration/plan output and NDJSON execution events while preserving
human CLI output by default. Keep machine events separate from diagnostic stderr. Events include
sequence, run/revision/job identity, stage, optional scene ID, timestamp, state, elapsed time,
artifact availability, warning/error code, and cost information where known. Completion requires
a validated published output, not merely a provider's completed status or denoising counter.

Rust's planning path must be side-effect-free with respect to paid calls and usable without a
provider key when saved assets suffice. Resolve the exact proposed revision before estimation.
Show costs as estimates with uncertainty and distinguish local work from paid generation.
The existing `--max-cost` is an estimate guard, not a guaranteed billing cap; budget checks must
account for retries/validation candidates and explain provider pricing uncertainty. Bind approval
to a plan/config/artifact hash and reject changed plans. Require new approval for expanded work.

Suggested same-origin API:

- `GET /api/capabilities`, `/api/settings`, `/api/runs`, `/api/runs/:id`.
- `POST /api/uploads`, `/api/runs`, `/api/runs/:id/revisions`, `/api/plans`.
- `POST /api/jobs` with approved plan ID/hash and idempotency key.
- `GET /api/jobs/:id`, `/api/jobs/:id/events` (SSE with Last-Event-ID replay).
- `POST /api/jobs/:id/cancel`, `/api/jobs/:id/resume` (explicit user action).
- `GET /api/artifacts/:id` with byte ranges for video/audio scrubbing and downloads.

Persist queued → running → succeeded / succeeded-with-warnings / failed / interrupted, plus
cancelling/cancelled states. Start with one active render/generation job and bounded logs/events;
keep API/SSE responsive during CPU-heavy work. Persist intent before launch and reconcile after
restart. Do not blindly retry accepted paid submissions when the outcome is unknown. Preserve
remote job IDs and resume polling where supported; surface ambiguous submissions for review.

Cancellation signals the child's process group, allows cleanup, then terminates descendants if
needed. Container shutdown must stop launching jobs and leave resumable state. Cancellation of
local execution is not a guarantee that a hosted provider stopped billing; report remote status
and use provider cancellation only where supported. Show actual stage counts/elapsed time rather
than invented overall percentages; optional local H3 diagnostics retain their real scope.

## 7. Docker delivery and security

Use a multi-stage image: locked Rust build (respect MSRV 1.88), locked Node frontend/server
build, and minimal Linux runtime with Node, Rust executable, ffmpeg/ffprobe (including libass),
DejaVu fonts, CA certificates, and pinned Python/Whisper dependencies in a dedicated venv.
Include the default Whisper model in the image or a reproducible build layer so basic timing
does not rely on an undocumented first-run install. Additional model downloads are explicit and
cached in a named volume. Model APIs still require network access; “self-contained” means no
host Cargo, Node, Python, ffmpeg, or virtualenv requirement, not offline AI generation.

Planned operator experience (commands become valid only after implementation):

```sh
mkdir -p out
docker compose up --build -d studio
# Open http://localhost:3000
docker compose run --rm cli --help
docker compose run --rm cli --from /data/out/<run> --dry-run
```

- Studio and CLI services use the same built image; CLI is an on-demand Compose profile.
  Bind `./out:/data/out` in both and set the working directory so default CLI output lands there.
- Bind HTTP to `127.0.0.1:3000` by default. Run with an init process and non-root host-compatible
  UID/GID; document Linux ownership and Docker Desktop behavior. No privileged mode, host root
  mounts, Docker socket, or host virtualenv/source/target directory is needed at runtime.
- Store SQLite, private job metadata, and sessions in a persistent named state volume; generated
  artifacts, revision manifests, and uploaded run inputs stay under the output mount. Keep
  credentials outside both public artifact routes and run manifests. Document consistent backup
  of state plus output, rescan/recovery, and that removing a container does not remove renders.
- Exclude `.env`, `out`, `.git`, `.venv`, `target`, and private files from image build context.
  Supply secrets at runtime; use `REELMAESTRO_*` for new app-specific environment variables.
- Health/readiness checks cover writable storage, database, executable and media dependencies;
  report provider configuration separately without making paid health-check calls.
- Optional H3 is an external service, not bundled GPU infrastructure. Container localhost is not
  host localhost: document an explicit host-gateway/Compose-network endpoint for Linux/Desktop.
  Never silently fall back from local video to a paid hosted provider.
- Use argument-array spawning without a shell. Reject path traversal and symlink escapes for
  reads and writes; whitelist artifact types, validate upload signatures/sizes and text lengths,
  restrict fetch schemes, and block private/link-local/metadata URL targets including redirects
  and DNS rebinding. Keep operator-approved H3 access separate from user article URL fetching.
- Protect even localhost against hostile websites: authenticated same-origin session bootstrap,
  Host/Origin checks, CSRF protection, no wildcard CORS, request limits, and CSP. Render prompts,
  metadata, logs, and provider errors as untrusted text. Remote access requires explicit secure
  authentication/TLS configuration and is not enabled by merely publishing another interface.
- Lock dependency versions, scan the image and Node dependencies, and extend existing Rust
  supply-chain checks without replacing them. Document third-party asset/library licenses.

## 8. Delivery sequence and acceptance gates

1. **Contracts and safe resume:** implement manifests, stable IDs, dependency plans, atomic
   publication, configuration schema, and structured events in Rust. Test old CLI behavior and
   legacy folders. Gate: edits cannot reuse the wrong scene or silently invoke unrelated stages.
2. **Container + headless backend:** reproducible image, mounts, uploads, SQLite queue, process
   supervision, API/SSE, artifact serving, and read-only library import. Gate: a fresh host with
   only Docker can run CLI help and complete an offline fixture render visible in host `out`.
3. **Design system + vertical slice:** branded light/dark/system shell, Library → Create →
   Activity → Preview, including empty/loading/error/warning states. Gate: submit, refresh,
   reconnect, inspect progress, play output, and download artifacts entirely in the browser.
4. **Editing and complete controls:** scene/chapter inspector, revision history, selective
   regeneration, music/video toggles, budget confirmation, and advanced feature parity. Gate:
   all configuration categories above are usable without terminal interaction; revision diff
   explains reuse and paid work before submission.
5. **Hardening and documentation:** accessibility, responsive visual review, recovery/security
   tests, container dependency policy, and operator guide. Update README (including “No Docker,
   no server, no dashboard”), `.env.example`, and CONTRIBUTING when those features actually ship.

Verification must use local fixtures/mock providers unless the user explicitly authorizes paid
or live provider workflows. Test provider call counts and artifact hashes, not only UI messages:

- Still preview → selected video upgrade generates only requested clips, retaining audio,
  timings, stills, poster, and music byte-for-byte.
- Music addition invokes only music generation; removal invokes no model. Render-only changes
  invoke no provider. One-scene regeneration invalidates only its real dependents.
- Deletion/reordering preserves stable scene ownership; narration-removal cost and retiming
  are explicit. Include YouTube chapter edits, silent mode, and old unmanifested output folders.
- Restart, SSE reconnect, duplicate submissions, concurrent tabs/CLI, cancellation, partial
  artifacts, disk-full, host edits, and ambiguous remote acceptance do not silently duplicate
  paid work or overwrite completed revisions.
- Test traversal/symlinks, SSRF/redirects, unauthorized access, hostile uploads, credential
  redaction, and valid byte-range media playback.
- Run existing Rust tests, render/music/video smoke checks, `cargo fmt --all`, Clippy, and
  dependency checks; add TypeScript type checks, backend integration tests, and Playwright
  browser tests using offline media fixtures.
- Capture and inspect rendered screens in both themes at desktop/tablet/mobile sizes, with
  reduced motion/transparency, keyboard navigation, empty library, active job, failure, and
  revision comparison. Check contrast and accessibility; retain representative reviewed
  screenshots with implementation results. No generated mockup substitutes for a working UI.

Definition of done: a user can install with Docker, configure providers safely, create and track
a video, preview it, edit scenes, add/remove music or selected video, approve only necessary
regeneration, compare versions, and inspect/download host-visible artifacts without invoking
the terminal after setup. The same Rust CLI remains usable natively and in the container.
