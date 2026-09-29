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
  stop               Stop services cleanly; preserve containers, volumes, and data
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

if [ -f "$SCRIPT_DIR/.env" ]; then
    # Source only the operator-owned repository file; never print its contents.
    set -a
    . "$SCRIPT_DIR/.env"
    set +a
fi

case "$command" in
    start|cli|whisper|ffmpeg|ffprobe)
        mkdir -p out
        docker compose --profile cli build
        ;;
esac

startup_banner() {
    host_ip=$(ip -4 route get 1.1.1.1 2>/dev/null | awk '{for(i=1;i<=NF;i++) if($i=="src") {print $(i+1); exit}}') || host_ip=
    if [ -z "$host_ip" ]; then
        host_ip=$(hostname -I 2>/dev/null | awk '{print $1}') || host_ip=
    fi
    binding=$(docker compose port studio "${REELMAESTRO_PORT:-3001}" 2>/dev/null | head -n 1) || binding=
    if [ -z "$binding" ]; then
        printf '\nStudio passed internal health checks, but Docker has no published port.\n' >&2
        printf 'Check for a port conflict, then recreate Studio with:\n' >&2
        printf '  docker compose up -d --wait --force-recreate studio\n' >&2
        printf 'Volumes and output will be preserved. Studio is not ready for browser access.\n' >&2
        return 1
    fi
    printf '\n============================================================\n'
    printf '  REEL MAESTRO STUDIO — Ready\n'
    printf '============================================================\n'
    printf '  Host IP       %s\n' "${host_ip:-Unavailable}"
    printf '  Published     %s\n' "$binding"
    case "$binding" in
        127.*|\[::1\]:*)
            printf '  Open Studio   http://localhost:%s\n' "${binding##*:}"
            printf '  Access        This computer only (LAN access is disabled)\n'
            ;;
        0.0.0.0:*|\[::\]:*)
            printf '  Open locally  http://localhost:%s\n' "${binding##*:}"
            printf '  Access        All interfaces; Studio Host/Origin rules still apply\n'
            ;;
        *) printf '  Published URL http://%s\n' "$binding" ;;
    esac
    printf '  Videos        %s/out\n' "$SCRIPT_DIR"
    printf '  Status        ./run.sh status\n'
    printf '  Stop safely   ./stop.sh\n'
    printf '============================================================\n\n'
}

case "$command" in
    start)
        docker compose up -d --wait studio
        startup_banner
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
        docker compose --profile cli stop --timeout 60
        ;;
esac
