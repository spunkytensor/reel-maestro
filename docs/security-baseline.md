# Spunky Tensor security: adoption status

Owner: Spunky Tensor maintainers. Supported versions remain the latest release and
`main` until 1.0, as specified in [SECURITY.md](../SECURITY.md). No licensing or
support contract is changed by this rollout.

Baseline: [ed53814ed23f76c11fa4a91f57f99de903c18bfc](https://github.com/spunkytensor/.github/commit/ed53814ed23f76c11fa4a91f57f99de903c18bfc).
This is partial implementation, not a compliance certification.

## Delivered checks and evidence

- [Spunky Tensor security](https://github.com/spunkytensor/reel-maestro/actions/workflows/public-repo-security.yml)
  calls the SHA-pinned shared Trivy 0.74.0 workflow on PRs, main pushes, and nightly
  at **09:09 UTC** (01:09 PST / 02:09 PDT). Source scans include development
  dependencies. Registry Cargo and npm lockfile package/version pairs are reconciled
  against the report; a missing package fails coverage.
- RustSec `cargo audit`, cargo-deny, the Rust CycloneDX generator, and existing npm
  audits remain. RustSec also runs nightly at 09:09 UTC. No scanner-equivalence
  claim or suppression translation was used to remove ecosystem coverage.
- Container CI rebuilds and scans its local linux/amd64 image nightly and on PRs
  and main pushes. It retains offline render/readiness/model checksum checks and
  requires representative OS, Python, and Node inventory entries. Image scans
  fail on High/Critical findings **including unfixed findings** and retain all
  severities. The image config digest identifies the local build; it is not a
  registry manifest digest or a scan of a previously shipped image.
- Source (`security-source`) and image (`security-runtime-linux-amd64`) Actions
  artifacts contain SPDX and CycloneDX SBOMs, full vulnerability JSON, Trivy version,
  subject identity, and checksums. They expire after 30 days. Download them from
  the corresponding workflow run; there is no durable release-SBOM download yet.
- Dependabot covers Cargo, npm, Python, Docker and GitHub Actions. The changed
  security workflows pin actions and use read-only tokens without publish secrets.
- Full-inventory Trivy scanning replaces the service-dependent dependency-review
  gate; no paid private-repository security add-on is required.

Nightly runs start only after merge to the default branch. Check the linked runs
for last successful scans; no successful nightly run is claimed by this PR.
The organization freshness report is separate rollout work and must confirm this
caller's path and alert on failed/missing scans older than 36 hours.

## Artifact-specific coverage and release gates

Current delivery paths include the Cargo source package, locally built CLI,
Studio source/build, and locally built Docker image. No GitHub releases were
listed during this rollout; no public image release manifest was identified.
Do not invent a digest or publish an image merely to feed the shared scanner.

Source Trivy detects Cargo.lock and studio/package-lock.json. It does **not**
detect docker/requirements-whisper.txt in the representative scan: installed
Python and OS coverage comes from the image scan. Source/build dependencies are
not a claim about exactly what remains linked into the release binary.

[THIRD_PARTY_NOTICES.txt](../THIRD_PARTY_NOTICES.txt) and full font/Whisper license
texts are included in source packaging. Notices and the Whisper license are copied
into `/app` in the image; existing full font licenses remain in Studio's
`/app/studio/web/dist/licenses/` and are served under `/licenses/`.
Existing installed package notices are preserved. Remaining release work:

1. Generate and review complete resolved third-party texts (ORT/ScanCode pilot or
   equivalent), including statically linked Rust, bundled frontend code, Python
   native libraries and Wolfi packages. cargo-deny license metadata is not this
   notice bundle. Review AGPL whisper-timestamped and FFmpeg GPL/LGPL source
   obligations, applicable upstream NOTICE files, fonts/subset provenance, model
   terms, and media/design asset provenance. Do not infer legal approval here.
2. Reconcile installed package metadata/build outputs fully, including Node itself,
   bundled native libraries, font and model hashes. The representative image
   assertions and nonempty SBOM gate are not exhaustive completeness checks.
3. Before publishing any release, archive both SBOM formats, checksums, source
   commit, generation time, tool versions, reviewed notices and license texts,
   and provenance bound to each exact artifact digest as durable release assets.
   Include required texts inside each distributed binary archive/web bundle too.
4. For each supported released container platform, register the public immutable
   platform-manifest digest in nightly shared-workflow calls. Rebuilding main
   does not rescan older distributed images. Add platform coverage beyond amd64
   when release platforms are defined; review end-of-life base OS explicitly.

## Maintainer/admin work still required

Verify Dependabot alerts/security updates, secret scanning/push protection, and
the working private reporting route. Verify branch required checks/reviews, workflow/policy
ownership, maintainer 2FA/access review, and periodic Scorecard as appropriate.
Hosted CodeQL may remain supplemental in public repositories, but is not required
by this private-compatible baseline.
These settings and owner/legal assignments are not changed by this PR. Existing
non-security workflows still have mutable action refs to migrate in review.

Maintainers own CVE triage: assign each finding an owner and remediation date.
No exceptions are enabled. Any future exception needs scoped package/version and
finding, evidence, owner, reviewer, tracking reference and expiry, plus a reviewed
validation mechanism. Revoke exposed credentials rather than only deleting them.
