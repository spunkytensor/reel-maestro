# Contributing to Reel Maestro

Thanks for helping improve Reel Maestro. This project aims to stay easy to run: one Rust CLI,
local media tooling, and a single OpenRouter API key for model calls.

The minimum supported Rust version is 1.88, as declared in `Cargo.toml`.

## Before you start

- Open an issue for large behavior changes so the approach can be discussed first.
- Keep pull requests focused and small enough to review.
- Do not commit secrets, generated reels, local virtual environments, or build outputs.
- If you reuse or adapt third-party code, prompts, assets, or documentation, include the
  required attribution and license/cross-reference in the PR description and repository files.

## Development setup

```bash
git clone https://github.com/spunkytensor/reel-maestro.git
cd reel-maestro
cp .env.example .env      # optional for live API runs; paste OPENROUTER_API_KEY
cargo build
```

Install local runtime tools as needed:

- `ffmpeg` and `ffprobe` for render-path tests and actual video assembly.
- Optional: `whisper-timestamped` for exact word-level caption timing. See the README's
  `whisper-timestamped` section for the `uv`-based installation path.

## Local checks

Run the cheap checks before opening a PR:

```bash
cargo fmt --all
cargo fmt --all --check
cargo clippy --all-targets --locked -- -D warnings
cargo deny check
cargo test
cargo build
cargo package --locked
cargo run -- --help
```

Render smoke tests are ignored by default because they require `ffmpeg`/`ffprobe` and produce
temporary media files:

```bash
cargo test render_smoke -- --ignored --nocapture
cargo test music_mix_smoke -- --ignored --nocapture
cargo test video_mode_smoke -- --ignored --nocapture
```

End-to-end runs call paid model APIs. Only run them intentionally, with your own
`OPENROUTER_API_KEY`, and note that they may incur charges.

Automated local H3 tests use bounded loopback HTTP fixtures, never OpenRouter or a GPU worker.
`cargo test` also exercises authenticated MP4 delivery with an FFmpeg-generated fixture, so
install `ffmpeg`/`ffprobe` before running that suite. CLI configuration tests isolate their
environment and do not read your `.env`. Run real H3 validation only with explicit operator
approval, without stopping unrelated jobs; retain the same manifest when testing resume.
Keep local bearer credentials, manifests, and generated media out of Git.

## Security artifacts

The CVE Audit workflow checks the Rust dependency lockfile on pushes, pull requests, and a weekly
schedule. It uploads its RustSec/CVE results as `cargo-audit.json`.

CI separately runs the dependency policy check and uploads a CycloneDX 1.5 SBOM as
`reelmaestro-sbom.cdx.json`.

To reproduce locally:

```bash
cargo install cargo-audit --version 0.22.2 --locked
cargo install cargo-cyclonedx --version 0.5.9 --locked
cargo install cargo-deny --version 0.19.8 --locked
mkdir -p target/security
cargo deny check
cargo audit --json > target/security/cargo-audit.json
cargo cyclonedx --format json --spec-version 1.5 --override-filename reelmaestro-sbom
mv reelmaestro-sbom.json target/security/reelmaestro-sbom.cdx.json
```

## Code style

- Prefer the existing single-binary, module-per-stage structure.
- Keep error messages actionable and include enough context to diagnose failed media/API steps.
- Avoid adding dependencies unless they materially simplify the CLI or media pipeline.
- Use `REELMAESTRO_*` for Reel Maestro-specific environment variables.
- Keep generated artifacts under `out/` or the system temp directory.

## Studio development

Studio requires Node.js 22.13+ in addition to the CLI media tools. See
[`studio/README.md`](studio/README.md) for local startup, storage, and current limitations.

```sh
cargo build --locked
npm --prefix studio ci
npm --prefix studio run build
npm --prefix studio test
npm --prefix studio run test:browser
npm --prefix studio run format
npm --prefix studio run format:check
npm --prefix studio audit
```

The browser suite needs Chromium (`CHROME` can select an installed executable, or run
`npm --prefix studio exec -- playwright-core install --with-deps chromium`). It starts a temporary
server, uses mock paid generation plus the real credential-free Rust revision engine and local ffmpeg fixtures, and retains screenshots under
`out/studio-verification/`. Inspect screenshots against `docs/design/` when changing visual UI.
Studio CI runs TypeScript, backend/browser tests, formatting, and npm dependency auditing alongside
the unchanged Rust checks. Never replace the fixture executable with a paid provider in CI.

`reelmaestro --studio-schema` reports the version-1 CLI argument inventory and availability;
`reelmaestro --studio-estimate` emits a version-1 fresh-generation cost estimate without an API
key or paid calls. Rust remains the authority for estimates/configuration. `--studio-inspect-run`
returns normalized stable scene IDs and a source fingerprint. `--revision-plan` consumes a versioned
request and returns a complete dependency/cost plan; `--revision-execute` and `--revision-recover`
require the exact persisted plan plus `--approval-hash`. `--events-json` emits structured NDJSON.
Revision tests cover stale/tampered inputs, copied artifact hashes, immutable publication, and
refusal to retry ambiguous paid work. Keep these contracts and browser validators in sync.
`--no-dotenv` disables both local and ancestor `.env` discovery; Studio always passes it so that
unapproved host-side environment-file changes cannot affect a submitted job.

Container build, network-disabled fixture render, readiness, dependency scan, and backup checks
are documented in [`docs/container.md`](docs/container.md). Never run live provider calls as a
substitute for offline verification without explicit authorization.

## Pull request expectations

1. Explain the user-facing change and why it is needed.
2. List the checks you ran, including any ignored smoke tests or live API runs.
3. Mention any costs, network calls, model changes, or new external dependencies.
4. Include attribution/cross-reference notes for any third-party material used.

## License

By contributing to this repository, you agree that your contributions are submitted under the
same [Apache License 2.0](LICENSE) unless you explicitly state otherwise in writing.
