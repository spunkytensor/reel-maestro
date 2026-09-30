#!/usr/bin/env bash
# Local Docker images cannot be passed between reusable workflow jobs.
set -euo pipefail
mkdir -p security-runtime
image=$(docker image inspect reel-maestro:ci --format '{{.Id}}')
jq -n --arg image "$image" --arg commit "$GITHUB_SHA" \
  --arg generated "$(date -u +%FT%TZ)" \
  '{local_image_config_digest: $image, commit: $commit, generated: $generated,
    platform: "linux/amd64", scope: "CI build, not a published release"}' \
  > security-runtime/subject.json
trivy image --image-src docker --scanners vuln --list-all-pkgs \
  --ignorefile /dev/null --ignore-unfixed=false --exit-code 0 \
  --format json --output security-runtime/trivy.json "$image"
trivy version --format json > security-runtime/trivy-version.json
trivy convert --format spdx-json --output security-runtime/sbom.spdx.json security-runtime/trivy.json
trivy convert --format cyclonedx --output security-runtime/sbom.cdx.json security-runtime/trivy.json
(cd security-runtime && sha256sum ./*.json > SHA256SUMS)
# Assert representative OS, Python and production Node coverage, not just nonempty output.
jq -e '
  [.Results[]?.Packages[]?.Name] as $names |
  all(["ffmpeg-9.0", "libass", "python-3.11", "nodejs-22", "torch", "openai-whisper", "whisper-timestamped", "fastify"][];
    . as $name | $names | index($name) != null)
' security-runtime/trivy.json
count=$(jq '[.Results[]?.Vulnerabilities[]? | select(.Severity == "HIGH" or .Severity == "CRITICAL")] | length' security-runtime/trivy.json)
echo "High/Critical findings: $count"
test "$count" -eq 0
