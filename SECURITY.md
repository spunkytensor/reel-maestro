# Security Policy

## Supported versions

Until Reel Maestro reaches 1.0, security fixes are provided for the latest release and the
`main` branch only. Older pre-1.0 releases may receive fixes at the maintainers' discretion.

## Reporting a vulnerability

Please do **not** open a public issue that contains vulnerability details, API keys, generated
media with sensitive content, or other secrets.

Use GitHub's private vulnerability reporting / Security Advisory flow for this repository. If
that is unavailable, open a minimal public issue asking for a private reporting channel, but do
not include exploit details or secrets in that issue.

Helpful reports include:

- Affected version or commit.
- Reproduction steps and expected impact.
- Whether the issue can expose `OPENROUTER_API_KEY` or other local secrets.
- Whether the issue involves command execution, path handling, URL fetching, generated media, or
  third-party model/provider behavior.

## Security scope

Examples of in-scope issues include:

- API key exposure or accidental logging of secrets.
- Command/path injection around `ffmpeg`, `ffprobe`, or `whisper_timestamped` execution.
- Unsafe handling of remote URLs, generated media, or local files.
- Dependency vulnerabilities affecting the CLI.
- Cases where generated artifacts may unexpectedly include sensitive inputs.

Reel Maestro depends on OpenRouter and selected model providers. Provider availability, content
policy decisions, model output rights, and model-side data handling are governed by those services.
