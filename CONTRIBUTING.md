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

## Security artifacts

CI runs Rust dependency policy checks and uploads two supply-chain artifacts on every run:

- `cargo-audit.json` — RustSec/CVE audit output from `cargo audit --json`.
- `reelmaestro-sbom.cdx.json` — CycloneDX 1.5 SBOM from `cargo cyclonedx`.

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

## Pull request expectations

1. Explain the user-facing change and why it is needed.
2. List the checks you ran, including any ignored smoke tests or live API runs.
3. Mention any costs, network calls, model changes, or new external dependencies.
4. Include attribution/cross-reference notes for any third-party material used.

## License

By contributing to this repository, you agree that your contributions are submitted under the
same [Apache License 2.0](LICENSE) unless you explicitly state otherwise in writing.
