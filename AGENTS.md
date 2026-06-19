# AGENTS.md

Guidance for AI coding agents working on Reel Maestro.

## Project overview

Reel Maestro is a Rust CLI (`reelmaestro`) that creates vertical short-form videos from a topic,
brief, script, URL, or previous run folder. The pipeline is: script planning, TTS, local word
timing, image generation, optional music/video generation, and local ffmpeg assembly.

## Important constraints

- Prefer small, focused changes that fit the existing module-per-stage structure in `src/`.
- Do not run live OpenRouter/API workflows unless the user explicitly asks; they can cost money.
- Never commit or print real API keys. `.env` is local only; `.env.example` is the public template.
- Reel Maestro-specific environment variables use the `REELMAESTRO_*` prefix.
- The minimum supported Rust version is 1.88, as declared in `Cargo.toml`; keep CI and docs in sync.
- Generated media belongs in `out/` or a temp directory, not in source control.
- Keep cross-references and attribution current when adding third-party code, prompts, assets, or
  documentation.

## Common commands

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

Use `cargo fmt --all` to format the entire workspace before handing work back. Use
`cargo fmt --all --check` only when verifying that formatting is already correct, such as in
CI. Do not manually reflow Rust code that `rustfmt` can format.

Ignored render-path checks require `ffmpeg`/`ffprobe` and produce temporary media:

```bash
cargo test render_smoke -- --ignored --nocapture
cargo test music_mix_smoke -- --ignored --nocapture
cargo test video_mode_smoke -- --ignored --nocapture
```

## Source map

- `src/main.rs` — CLI flags and orchestration.
- `src/config.rs` — CLI/env/default configuration resolution.
- `src/openrouter.rs` — OpenRouter HTTP client.
- `src/script.rs`, `src/tts.rs`, `src/images.rs`, `src/music.rs`, `src/video.rs` — model stages.
- `src/transcribe.rs`, `src/captions.rs` — local timing and ASS caption generation.
- `src/ffmpeg.rs`, `src/assemble.rs` — local render, muxing, poster, and smoke tests.

## Documentation expectations

When user-facing flags, env vars, costs, output files, or release processes change, update
`README.md`, `.env.example`, and `CONTRIBUTING.md` as applicable.

CI also checks dependency policy with `cargo deny check` and generates Rust supply-chain artifacts
with `cargo audit --json` and `cargo cyclonedx --format json --spec-version 1.5`; keep those
workflow commands in sync with `CONTRIBUTING.md` if the tooling changes.

## Rust source headers

Every Rust source file should start with:

```rust
// Copyright 2026 Spunky Tensor
// SPDX-License-Identifier: Apache-2.0
```
