#!/bin/sh
# Offline launcher/config regression: only synthetic credentials are used.
set -eu
ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
TEMP=$(mktemp -d)
trap 'rm -rf "$TEMP"' EXIT HUP INT TERM
cp "$ROOT/run.sh" "$ROOT/compose.yaml" "$TEMP/"
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
EOF
chmod +x "$TEMP/bin/docker"
CHECK_LOG="$TEMP/calls" PATH="$TEMP/bin:$PATH" "$TEMP/run.sh" cli --help
test "$(sed -n '1p' "$TEMP/calls")" = 'compose --profile cli build'
test "$(sed -n '2p' "$TEMP/calls")" = 'compose run --rm cli --help'
OPENROUTER_API_KEY=synthetic-launcher-key docker compose --project-directory "$TEMP" -f "$TEMP/compose.yaml" config --format json |
  python3 -c 'import json,sys; c=json.load(sys.stdin); assert all(s["environment"]["OPENROUTER_API_KEY"]=="synthetic-launcher-key" and s["environment"]["REELMAESTRO_TEXT_MODEL"]=="synthetic model with spaces" and s["environment"]["REELMAESTRO_OUT_DIR"]=="/data/out" for s in c["services"].values())'
rm "$TEMP/.env"
OPENROUTER_API_KEY=synthetic-launcher-key REELMAESTRO_TEXT_MODEL='synthetic model with spaces' REELMAESTRO_PORT=3339 CHECK_LOG="$TEMP/calls" PATH="$TEMP/bin:$PATH" "$TEMP/run.sh" status
OPENROUTER_API_KEY=synthetic-launcher-key docker compose --project-directory "$TEMP" -f "$TEMP/compose.yaml" config --format json |
  python3 -c 'import json,sys; c=json.load(sys.stdin); assert all(s["environment"]["OPENROUTER_API_KEY"]=="synthetic-launcher-key" for s in c["services"].values())'
echo 'Launcher .env export, runtime injection, and missing-file checks passed.'
