---
name: rust-conventions
description: Enforces Reel Maestro Rust source conventions from AGENTS.md
severity-default: medium
tools: [Grep, Read]
---

Check changed Rust files under `src/` and `tests/` against the project
conventions documented in `AGENTS.md`.

Flag the following:

- Any new or modified `.rs` file that does not begin with the required header:

  ```rust
  // Copyright 2026 Spunky Tensor
  // SPDX-License-Identifier: Apache-2.0
  ```

- Reel Maestro environment variables that do not use the `REELMAESTRO_*` prefix.
- Use of language or standard-library features that require a Rust edition newer
  than the minimum supported version (1.88, as declared in `Cargo.toml`).
- `.unwrap()` / `.expect()` / `panic!` introduced on fallible paths that handle
  user input, file I/O, or network responses, where a `Result` should be
  propagated instead. Ignore these in tests and in genuinely infallible cases.
- New per-stage logic that does not fit the existing module-per-stage structure
  in `src/` (e.g. mixing TTS, image, or ffmpeg concerns into unrelated modules).

## Documentation and comments

Prefer liberal, meaningful commenting. Flag the following:

- Public items (`pub fn`, `pub struct`, `pub enum`, `pub trait`, and public
  modules) that lack `///` doc comments explaining their purpose, parameters,
  and error/return semantics.
- Non-obvious logic — pipeline ordering, timing math, ffmpeg/OpenRouter argument
  construction, retry/backoff, unit conversions — that has no explanatory `//`
  comment describing *why* (not merely *what*).
- `unsafe` blocks without a `// SAFETY:` comment justifying the invariants.
- Outdated or misleading comments that no longer match the changed code.

Do NOT demand comments on trivial, self-explanatory code, and do not flag simple
restatements of the code as good comments.

## Formatting and lints

The project standardizes on `rustfmt` and `clippy` (see `AGENTS.md`). Flag the
following:

- Code that appears hand-reflowed or otherwise not `rustfmt`-clean, such that
  `cargo fmt --all --check` would fail. Recommend running `cargo fmt --all`
  rather than nitpicking individual whitespace.
- New `#[allow(...)]` / `#![allow(...)]` attributes that suppress `clippy` or
  compiler warnings without a comment justifying why, given CI runs
  `cargo clippy --all-targets --locked -- -D warnings`.
- Obvious clippy anti-patterns (needless clones, redundant `.to_string()`,
  manual `map`/`unwrap_or` that has a standard combinator, `&Vec<T>` where
  `&[T]` suffices, etc.).

## Security and supply chain

CI enforces dependency policy with `cargo deny check` and generates supply-chain
artifacts via `cargo audit --json` and `cargo cyclonedx`. Flag the following:

- New or updated dependencies in `Cargo.toml` / `Cargo.lock` that are
  unmaintained, duplicated, or likely to violate `deny.toml` (license,
  advisories, or banned-crate rules). Recommend running `cargo deny check` and
  `cargo audit`.
- Introduction of a dependency where the standard library or an existing crate
  already covers the need.
- Network, filesystem, or subprocess (`std::process::Command`, e.g. ffmpeg)
  calls that interpolate unvalidated user or model input into paths, URLs, or
  command arguments (command-injection / path-traversal risk).
- Fallible I/O or network results that are silently ignored (`let _ =`,
  discarded `Result`) on paths where failure should surface an error.

Do NOT re-flag pure formatting issues once you have noted that `cargo fmt` should
be run.
