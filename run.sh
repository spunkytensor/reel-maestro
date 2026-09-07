#!/bin/sh

set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
cd "$SCRIPT_DIR"

usage() {
    cat <<'EOF'
Usage: ./run.sh [command] [arguments...]

Commands:
  start              Build and start Studio (default)
  cli [args...]      Run the native reelmaestro CLI
  whisper [args...]  Run whisper_timestamped
  ffmpeg [args...]   Run ffmpeg
  ffprobe [args...]  Run ffprobe
  status             Show Compose service status
  stop               Stop and remove the Compose services
  help               Show this help
EOF
}

command=${1:-start}
if [ "$#" -gt 0 ]; then
    shift
fi

case "$command" in
    help|-h|--help)
        usage
        exit 0
        ;;
    start|cli|whisper|ffmpeg|ffprobe|status|stop)
        ;;
    *)
        printf 'Unknown command: %s\n\n' "$command" >&2
        usage >&2
        exit 2
        ;;
esac

case "$command" in
    start|cli|whisper|ffmpeg|ffprobe)
        mkdir -p out
        docker compose --profile cli build
        ;;
esac

case "$command" in
    start)
        docker compose up -d --wait studio
        ;;
    cli)
        docker compose run --rm cli "$@"
        ;;
    whisper)
        docker compose run --rm --entrypoint whisper_timestamped cli "$@"
        ;;
    ffmpeg)
        docker compose run --rm --entrypoint ffmpeg cli "$@"
        ;;
    ffprobe)
        docker compose run --rm --entrypoint ffprobe cli "$@"
        ;;
    status)
        docker compose ps
        ;;
    stop)
        docker compose down
        ;;
esac
