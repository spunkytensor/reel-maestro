---
name: generated-media
description: Ensures generated media and docs stay consistent with project rules
severity-default: medium
tools: [Grep, Read]
---

Reel Maestro generates audio, images, and video during runs. These artifacts and
the docs that describe them have specific rules in `AGENTS.md`.

Flag the following:

- Generated media (`.mp3`, `.wav`, `.mp4`, `.png`, `.jpg`, poster frames, etc.)
  written outside of `out/` or a temp directory, or such files newly added to
  source control.
- Code that hardcodes an output path instead of writing under the configured
  `out/` directory or a temporary directory.
- Changes to user-facing CLI flags, environment variables, costs, or output
  files that are not reflected in `README.md`, `.env.example`, and
  `CONTRIBUTING.md` as applicable.
- Changes to release, dependency-policy, or supply-chain tooling (`cargo deny`,
  `cargo audit`, `cargo cyclonedx`) in CI that are not mirrored in
  `CONTRIBUTING.md`.
- New third-party code, prompts, assets, or documentation added without
  corresponding attribution or cross-reference updates.
