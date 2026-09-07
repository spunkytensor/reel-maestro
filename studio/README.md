# Reel Maestro Studio

A local-first browser interface using the [approved Studio design system](../docs/design/DESIGN_GUIDE.md).
The interface covers creation, immutable revisions, selective regeneration, and local exports.
See [the execution and acceptance checklist](IMPLEMENTATION.md) for scope and verification.

## Run locally

For the containerized Studio (Docker + Compose only), run `./run.sh` from the repository root
and open <http://localhost:3000>. The launcher builds the image, including Whisper and ffmpeg,
then waits for Studio readiness. See [container operations](../docs/container.md) for runtime
credentials and the `cli`, `whisper`, `ffmpeg`, and `ffprobe` subcommands.

For native development without Docker:

Requirements: Linux, Node.js 22.13 or newer, Rust 1.88+, ffmpeg/ffprobe and the normal
[CLI media dependencies](../README.md#requirements). Node 20 cannot run Studio's SQLite store.

From the repository root:

```sh
cargo build --locked
npm --prefix studio ci
npm --prefix studio run build
npm --prefix studio run server
# Open http://localhost:3000
```

Studio serves the built app and API on one origin and listens on loopback by default. It does not
start generation merely because you open it. Supply `OPENROUTER_API_KEY` in the server environment,
describe a video, and review the estimate before approving paid work. Settings contains theme,
generation defaults, saved-video location/rescan, and application information—not credentials.
A configured key is **not** a verified provider connection. Previously saved private credentials
remain in the state directory and take precedence over environment keys.

Studio does not automatically load the repository `.env`. Supply server settings through the
process environment; export `OPENROUTER_API_KEY` when starting
the server. Generation subprocesses run in isolated directories with `--no-dotenv`, so neither
local nor ancestor `.env` files can change an approval; the environment is bound to the estimate.
Studio's narration/caption toggles take precedence over their `REELMAESTRO_NO_*` environment
defaults. Other operator model and voice overrides still apply. Changing generation configuration or
credentials invalidates outstanding approvals.

The native CLI remains independent and keeps its existing `.env` behavior. For a self-contained
installation, see [Docker setup, security, and backups](../docs/container.md).

## What works

- Browse/search/filter existing videos under `out/`, including incomplete/damaged projects.
- Play and scrub completed videos, explore scene images with arrow keys, inspect words/image/motion
  direction, and listen to available narration/music.
- Download original MP4s, posters, captions, and description/chapter documents. Studio does not
  re-encode, add a watermark, or claim to create an export when downloading an existing file.
- Create from a topic, pasted brief, verbatim script, uploaded text, or fetched article URL. Choose format, quality, widescreen length,
  narration/voice/pace, music, all-scene animation, captions, and an estimate guard.
- Review itemized Rust estimates and explicitly approve a single durable job. Duplicate approval
  cannot launch the same plan twice. Activity reconnects through SSE and work survives page refresh.
- Cancel local work; interrupted paid work is never automatically retried.
- Edit scene words, image/motion descriptions, ordering, deletion, chapters, characters, locations,
  and poster concepts. Drafts autosave in this browser and support undo/redo.
- Explicitly keep/regenerate individual stills and clips; animate selected scene IDs, remove or
  regenerate music, and customize models, timing, caption styling, and finishing. Upload reference
  images, watermarks, or a soundtrack. Blank revision settings inherit the existing version.
- Review reuse/regenerate/remove/restyle actions and costs before publishing an immutable version.
  Unknown legacy provenance requires an explicit trust choice. Source/configuration changes reject
  stale approvals; originals are never modified. Compare versions with side-by-side playback.
- Export a new H.264 MP4 using Web, Social, or Pro presets at the current dimensions. Arbitrary codec
  and bitrate selection is not part of v1. Existing-artifact downloads do not re-encode anything.
- Explicitly resume recoverable interrupted revisions from Activity, using the exact approved plan.
  Ambiguous paid acceptance requires operator resolution rather than a blind retry.
- Light/dark/system theme, reduced transparency/motion, browser-local generation defaults.

Public sharing, remote authentication/TLS, arbitrary executables/server paths, arbitrary export
codecs, and automatic paid retries are deliberately unavailable. Advanced browser controls expose
the supported typed model/media configuration, not unrestricted command-line execution.

## Storage and recovery

| Setting | Default | Purpose |
| --- | --- | --- |
| `REELMAESTRO_PORT` | `3000` | HTTP port |
| `REELMAESTRO_HOST` | `127.0.0.1` | Bind address; use a specific LAN IPv4 address for trusted-network access |
| `REELMAESTRO_OUT_DIR` | repository `out/` | Read-only imports and new generated media |
| `REELMAESTRO_STATE_DIR` | repository `.studio-state/` | SQLite plans/jobs/events and private credentials |
| `REELMAESTRO_BINARY` | repository `target/debug/reelmaestro` | Operator-selected fixed CLI executable |

Use absolute paths for overrides. Browser requests never choose server paths or executables.
Keep state outside output and outside any web-served directory. LAN binding is opt-in:
everyone who can reach the listener can use Studio, including initiating paid generation.
Use only a trusted network; do not expose it to the internet or a public reverse proxy.
Remote authentication/TLS is not implemented. Connect using the exact configured address and port.
Only a trusted local OS user should have access to these directories.

New work lives under `out/.studio-jobs/<job-id>/<video-folder>/`. Completed artifacts are ordinary
CLI files. Original imported folders are never rewritten. The SQLite database records intent before
launch; the server validates a positive-duration video stream with ffprobe before reporting success.
Activity reports native stages and warnings without inventing overall progress percentages.
Revisions live under the original run's `revisions/<id>/`, with a manifest recording artifact hashes
and dependencies. Hidden drafts are not completed library entries.

`GET /api/ready` returns 200 only when output/state are writable, SQLite is usable, and the CLI,
ffmpeg, ffprobe, and Whisper checks pass; otherwise it returns 503. Component statuses are included.
Provider configuration is reported separately and never triggers a paid readiness call. The endpoint
does not require a session, but the same Host/Origin rules still apply.

Stop Studio before backing up **both** the output and state directories. Removing only the state
directory loses job/approval history and stored credentials, not generated media. Only one Studio
process may use a state directory. On restart, unfinished work is marked interrupted and stays
stopped. Activity offers recovery only for eligible revisions, preserving the exact plan/hash and
accepted remote IDs. Ambiguous submissions remain blocked. For fresh runs, review remote work and
use the native CLI against the existing folder. Cancellation does
not guarantee cancellation of provider billing. Native CLI recovery should target the existing
folder and preserve any accepted local-video job records.

## Development and verification

```sh
npm --prefix studio run typecheck
npm --prefix studio test
npm --prefix studio run build
npm --prefix studio run test:browser
npm --prefix studio run format
npm --prefix studio audit
```

Build the native CLI with `cargo build --locked` before browser tests. The browser suite starts its
own real API servers over temporary output/state. Paid creation paths use a fake executable;
revision tests use the real credential-free Rust engine and reused images, proving immutable output
and stable-ID media mapping through an actual local ffmpeg render. It also checks uploads, URL
ingestion with an injected offline fetcher, review, comparison, explicit recovery controls, playback,
downloads, budget approval, keyboard navigation, theme persistence, reduced transparency,
and desktop/tablet/mobile layouts. It uses the installed Playwright Chromium, `CHROME` if supplied,
or the Chromium cache used by the existing mockup renderer. To install its browser:

```sh
npm --prefix studio exec -- playwright-core install --with-deps chromium
```

Screenshots go to ignored `out/studio-verification/`. Tests do not use real keys or paid APIs.
CI runs the same offline checks. For styling work, keep the built server running and use
`npm --prefix studio run dev` in another terminal to rebuild on changes, then refresh the browser.
This keeps development on the same authenticated origin instead of weakening Origin checks for
a second development server.

## Attribution

Studio imports the repository's approved `docs/design/src/studio.css`, which bundles Inter and
JetBrains Mono. Their full SIL Open Font License notices are distributed in `web/public/licenses/`
and exposed in Settings. The source notices come from
[Inter](https://github.com/rsms/inter/blob/master/LICENSE.txt) and
[JetBrains Mono](https://github.com/JetBrains/JetBrainsMono/blob/master/OFL.txt).
Browser tests use the same local stills as the mockups; those test media are not bundled into the app.
The operator-supplied transparent Spunky Tensor logo is bundled as `web/public/logo.png` and
displayed in the lower-right corner of Studio. It is UI branding, not an export watermark.
