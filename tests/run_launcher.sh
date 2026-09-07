#!/bin/sh
# Offline launcher/config regression: only synthetic credentials are used.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
TEMP=$(mktemp -d)
trap 'rm -rf "$TEMP"' EXIT HUP INT TERM
cp "$ROOT/run.sh" "$ROOT/stop.sh" "$ROOT/compose.yaml" "$TEMP/"
mkdir "$TEMP/bin"
cat > "$TEMP/.env" <<'EOF'
OPENROUTER_API_KEY=synthetic-launcher-key
REELMAESTRO_TEXT_MODEL='synthetic model with spaces'
REELMAESTRO_PORT=3339
EOF
cat > "$TEMP/bin/docker" <<'EOF'
#!/bin/sh
set -eu
test "$OPENROUTER_API_KEY" = synthetic-launcher-key
test "$REELMAESTRO_TEXT_MODEL" = 'synthetic model with spaces'
test "$REELMAESTRO_PORT" = 3339
printf '%s\n' "$*" >> "$CHECK_LOG"
case "$*" in
    'compose port studio 3339') printf '%s\n' "${CHECK_BINDING-127.0.0.1:3340}" ;;
    'compose up -d --wait studio') exit "${CHECK_START_EXIT:-0}" ;;
esac
EOF
chmod +x "$TEMP/bin/docker"
CHECK_LOG="$TEMP/calls" PATH="$TEMP/bin:$PATH" "$TEMP/run.sh" cli --help
test "$(sed -n '1p' "$TEMP/calls")" = 'compose --profile cli build'
test "$(sed -n '2p' "$TEMP/calls")" = 'compose run --rm cli --help'
CHECK_LOG="$TEMP/start-calls" PATH="$TEMP/bin:$PATH" "$TEMP/run.sh" > "$TEMP/banner"
grep -q 'REEL MAESTRO STUDIO.*Ready' "$TEMP/banner"
grep -q 'Host IP' "$TEMP/banner"
grep -q 'http://localhost:3340' "$TEMP/banner"
grep -q 'LAN access is disabled' "$TEMP/banner"
grep -q './stop.sh' "$TEMP/banner"
if grep -q 'synthetic-launcher-key' "$TEMP/banner"; then exit 1; fi
if CHECK_START_EXIT=1 CHECK_LOG="$TEMP/failure-calls" PATH="$TEMP/bin:$PATH" "$TEMP/run.sh" > "$TEMP/failed-banner"; then
    echo 'Failed startup must not succeed' >&2
    exit 1
fi
if grep -q 'Ready' "$TEMP/failed-banner"; then exit 1; fi
if CHECK_BINDING='' CHECK_LOG="$TEMP/no-port-calls" PATH="$TEMP/bin:$PATH" "$TEMP/run.sh" > "$TEMP/no-port-banner" 2> "$TEMP/no-port-error"; then
    echo 'Missing published port must fail startup verification' >&2
    exit 1
fi
if grep -q 'Ready' "$TEMP/no-port-banner"; then exit 1; fi
grep -q 'Docker has no published port' "$TEMP/no-port-error"
cat "$TEMP/banner"
mkdir -p "$TEMP/out"
printf 'retained output\n' > "$TEMP/out/sentinel"
CHECK_LOG="$TEMP/stop-calls" PATH="$TEMP/bin:$PATH" "$TEMP/stop.sh"
test "$(cat "$TEMP/stop-calls")" = 'compose --profile cli stop --timeout 60'
test "$(cat "$TEMP/out/sentinel")" = 'retained output'
test -f "$TEMP/.env"
OPENROUTER_API_KEY=synthetic-launcher-key docker compose --project-directory "$TEMP" -f "$TEMP/compose.yaml" config --format json |
  python3 -c 'import json,sys; c=json.load(sys.stdin); assert all(s["environment"]["OPENROUTER_API_KEY"]=="synthetic-launcher-key" and s["environment"]["REELMAESTRO_TEXT_MODEL"]=="synthetic model with spaces" and s["environment"]["REELMAESTRO_OUT_DIR"]=="/data/out" for s in c["services"].values())'
rm "$TEMP/.env"
OPENROUTER_API_KEY=synthetic-launcher-key REELMAESTRO_TEXT_MODEL='synthetic model with spaces' REELMAESTRO_PORT=3339 CHECK_LOG="$TEMP/calls" PATH="$TEMP/bin:$PATH" "$TEMP/run.sh" status
OPENROUTER_API_KEY=synthetic-launcher-key docker compose --project-directory "$TEMP" -f "$TEMP/compose.yaml" config --format json |
  python3 -c 'import json,sys; c=json.load(sys.stdin); assert all(s["environment"]["OPENROUTER_API_KEY"]=="synthetic-launcher-key" for s in c["services"].values())'
echo 'Launcher export, runtime injection, missing-file, and non-destructive stop checks passed.'
