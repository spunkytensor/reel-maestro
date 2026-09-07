# Studio implementation and acceptance plan

The visual authority is [the design guide](../docs/design/DESIGN_GUIDE.md); current behavior
is documented in [the Studio guide](README.md). This checklist tracks actual
delivery, not mockup completion.

## Execution order

1. **Machine contract:** expose versioned CLI capabilities and itemized, keyless, side-effect-free
   fresh-generation estimates derived from Rust. Preserve native CLI behavior. Verify subprocess
   tests, schema coverage, and no output creation/network calls.
2. **Local service:** read-only import of existing output, safe media streaming, write-only
   credentials, approved plans, durable single-worker jobs, cancellation and reconnect. Verify
   with temporary fixtures and fake executables, never live providers.
3. **Working design-system slice:** React/TypeScript screens using the approved CSS tokens:
   Start, Projects, scene/media inspection, activity, downloads, and Settings. All controls must
   either work or clearly explain why an operation is unavailable. Test keyboard navigation,
   themes, reduced transparency, persistence, mobile layout, error/empty states and downloads.
4. **Safe editing prerequisite:** stable scene ownership, content/dependency fingerprints,
   immutable versions, atomic publication, host-edit detection, CLI locks, explicit reuse choices,
   and approval-bound selective execution. Only then enable scene editing, apply changes, version
   comparison, and local export restyling. This is not fulfilled by editing script.json and resuming.
5. **Complete delivery:** typed advanced-control parity, uploads and safe URL ingestion,
   structured per-stage events, resumable paid jobs, container distribution and recovery/security
   gates from the full plan. These are separate acceptance gates, not implied by a working UI.

## Delivered checklist

- [x] Version-1 native schema, fresh estimates, normalized inspection, revision plan/execute/recover,
  stable scene IDs, dependency fingerprints, full-plan approval validation, and structured events.
- [x] Immutable versions: exclusive locks, verified byte copies, hidden draft work, validated video
  output, manifests, atomic publication, source/config drift rejection, and safe explicit recovery.
- [x] Single-worker durable API queue, plan-bound idempotency, interruption/cancellation escalation,
  bounded persistent SSE replay, readiness, exact Host/Origin/session/CSRF protections.
- [x] Rooted existing library including nested Studio jobs/revision families, ffprobe-backed status,
  byte-range playback/HEAD, companion downloads, and hidden/symlink exclusion.
- [x] Typed advanced controls, signed uploads with signature/size checks, constrained server-owned
  paths, and DNS-pinned article fetching with private-address/redirect/byte/time restrictions.
- [x] Scene/chapter/entity/poster editing, explicit per-scene keep/regenerate/animation, reorder and
  deletion, music actions, browser-local drafts, undo/redo, and stale-draft rejection.
- [x] Explicit action/cost review, version comparison, Web/Social/Pro H.264 export presets, and
  recoverable Activity actions without automatic paid retries.
- [x] Light/dark/system themes, reduced transparency/motion, local preferences, keyboard interaction,
  desktop/tablet/mobile layout, and rendered visual inspection.
- [x] Non-root pinned multistage Docker image, persistent volumes, same-image CLI, offline readiness,
  network-disabled host-visible render, dependency scanning, container CI and operator documentation.

## Verification evidence

- Rust: full enabled suite (140 tests), strict Clippy, formatting, dependency policy; real offline
  ffmpeg revision execution verifies copied still hashes, reordered ownership, removed music,
  inherited configuration, source immutability, manifest output, and terminal artifact identity.
- Backend: 29 offline integration tests covering sessions/CSRF, validation/uploads/URL restrictions,
  estimates and limits, stale approvals, idempotency, SSE, nested revision families, exact run IDs,
  interruption/recovery/ambiguity refusal, signal shutdown, state locks, and media ranges.
- Browser: actual bundled UI + real API; mock paid generation and real credential-free Rust local
  revision execution. Covers draft persistence/undo/reorder, upload/article input, cost approval,
  publication, playback/download, original byte identity, version comparison, recovery confirmation,
  themes, accessibility interactions, and responsive layouts.
- Docker: build, CLI help, healthy offline components with no provider configured, forged Host
  rejection, non-root runtime/model checksum, network-none H.264 fixture, and HIGH/CRITICAL scan.

Screenshots are retained in ignored `out/studio-verification/`. Live provider behavior is deliberately
not verified: no real credentials or paid generation were used for acceptance checks.

## Intentional v1 boundaries

Public sharing/authentication/TLS, arbitrary executable/server-path selection, arbitrary codecs or
bitrates, and automatic paid retries are not enabled. LAN binding is opt-in and only appropriate on
a trusted network. Provider configuration means a key is present, not that a paid health check passed.
Recovery is limited to exact previously approved work with safe native reconciliation; ambiguous
acceptance requires an operator. Existing downloads do not silently re-encode or watermark media.
