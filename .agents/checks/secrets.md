---
name: secrets
description: Flags leaked API keys, secrets, or credentials in source, tests, or docs
severity-default: critical
tools: [Grep, Read]
---

Reel Maestro talks to OpenRouter and other paid APIs. Real credentials must never
land in the repository.

Flag any of the following:

- Hardcoded API keys, tokens, or secrets (e.g. `sk-`, `sk-or-`, bearer tokens,
  long base64/hex secrets) in `src/`, `tests/`, examples, or documentation.
- `println!`, `eprintln!`, `dbg!`, `tracing`, or `log` statements that print an
  API key, token, or the raw value of a secret environment variable.
- New secret values committed to `.env` or any tracked file. Only `.env.example`
  should exist in source control, and it must contain placeholders, not real
  values.
- Test fixtures or snapshots that embed real-looking credentials.

Do NOT flag placeholder values in `.env.example` or obvious dummy strings such as
`sk-xxxx`, `your-api-key-here`, or `changeme`.
