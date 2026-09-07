# Container installation and operations

The Docker image is the complete Reel Maestro Studio runtime: the React application, Fastify
server, Rust CLI, ffmpeg/ffprobe with libass, DejaVu fonts, CA roots, and a dedicated Python 3.11
Whisper environment. Host Cargo, Node, Python, ffmpeg, source code, and virtual environments are
not mounted or required at runtime.

## Install and start

Install Docker Engine with the Compose plugin, then run from the repository root:

```sh
mkdir -p out
docker compose up --build -d studio
docker compose ps
```

Open <http://localhost:3000>. The server listens on all interfaces *inside* its container, but
Compose publishes it only on host loopback (`127.0.0.1`) by default. This preserves the exact
`Host` and `Origin` checks for `localhost:3000`; it does not expose Studio to the LAN. To avoid a
local port conflict without changing the container's security checks:

```sh
REELMAESTRO_PORT=3300 docker compose up -d studio
# Open http://localhost:3300
```

The Studio Settings screen accepts an OpenRouter key through its write-only API and stores it in
the private state volume. The value is never returned by the API or written to `out`. For a
one-off CLI invocation, pass the secret only to that container:

```sh
docker compose run --rm -e OPENROUTER_API_KEY cli --topic "octopus cognition"
```

Do not put credentials in the image or build arguments. A repository `.env` is excluded from the
build context, but Compose itself reads `.env` for variable substitution; prefer an exported
shell variable, a protected environment file supplied explicitly, or Studio's write-only setting.

## CLI and files

Studio and the CLI use the same image. The working directory is `/data/out`, backed by host
`./out`, so the CLI's default output and every Studio render remain visible on the host:

```sh
docker compose run --rm cli --help
docker compose run --rm cli --from /data/out/<run> --dry-run
```

The containers run without root privileges, Linux capabilities, privilege escalation, a Docker
socket, or host source/tool directories. The default image user is UID/GID 1000. On Linux, build
for the account that owns `./out` when those IDs differ:

```sh
export REELMAESTRO_UID="$(id -u)"
export REELMAESTRO_GID="$(id -g)"
mkdir -p out
docker compose build
docker compose up -d studio
```

Keep those variables consistent for later Compose commands. Existing mismatched files can be
fixed once on the host with `sudo chown -R "$(id -u):$(id -g)" out`. Docker Desktop shares files
through its VM and normally maps ownership automatically; leave the defaults unless writes fail.

`reel-maestro_state` stores SQLite, approvals, private job metadata, and saved credentials.
`reel-maestro_models` stores Whisper models. Removing or replacing a container does not remove
either named volume or host renders. `docker compose down -v` **does delete the named volumes**.

The default Whisper `base` checkpoint is downloaded during the reproducible image build and
verified by SHA-256. It is copied into a new model volume automatically, so basic local timing
does not download a model on first use. Selecting another `--whisper-model` explicitly downloads
and caches it in the model volume; review its size and license before doing so.

## Health and offline verification

The image healthcheck calls the local readiness endpoint. Readiness checks writable output/state,
SQLite, the Rust executable, ffmpeg/ffprobe, and Whisper without contacting OpenRouter or another
provider. Provider configuration is informational and never makes a paid health-check request.

Run the image's no-network fixture to exercise ffmpeg and the Rust resume/render path. It creates
a synthetic vertical video in the host mount and makes no model-provider calls:

```sh
docker compose run --rm --entrypoint /usr/local/lib/reelmaestro/offline-fixture.sh cli
ffprobe out/docker-offline-fixture/reel.mp4
```

## Optional local H3 service

H3 is external GPU infrastructure and is not bundled. Container `localhost` means the container,
not the host. A host service can be reached on Linux and Docker Desktop through the Compose
host-gateway mapping:

```sh
docker compose run --rm cli \
  --from /data/out/<run> --video --video-provider local \
  --video-base-url http://host.docker.internal:8088
```

If H3 is another Compose service on the same explicit network, use its service DNS name instead,
for example `http://h3:8088`. Keep H3 operator-approved and authenticated as appropriate. Reel
Maestro rejects local-only controls unless `--video-provider local` is explicit and never silently
falls back to a paid hosted video provider.

## Backup and recovery

Stop Studio briefly so SQLite, its WAL, and output files form one consistent point-in-time backup:

```sh
docker compose stop studio
tar -C . -czf reel-maestro-out.tgz out
docker run --rm \
  -v reel-maestro_state:/source:ro -v "$PWD":/backup \
  busybox:1.37.0 tar -C /source -czf /backup/reel-maestro-state.tgz .
docker compose start studio
```

Protect both archives as sensitive: state can contain credentials and private job metadata, while
output can contain uploaded inputs and generated media. Restore both from the same backup while
Studio is stopped. If state is lost but `out` remains, Studio rescans safe run folders for library
display; queue history, approvals, sessions, and saved credentials cannot be reconstructed from
public artifacts. Never copy a live SQLite file without its WAL or while writes continue.

## Network and security

AI generation requires outbound access to configured providers; URL input may fetch a requested
article. Self-contained means local tooling is included, not that hosted generation is offline.
Keep the default loopback publication. Publishing `0.0.0.0` does not add TLS, authentication, or
safe remote access and is unsupported as an exposure mechanism. Put an explicitly configured
authenticated TLS gateway in front only after reviewing proxy Host/Origin behavior.

No `.env`, `.git`, `out`, `target`, local virtualenv, source-control metadata, or private media is
sent in the Docker build context. Inspect effective configuration before startup with
`docker compose config` and do not paste its output publicly if you add secret-bearing overrides.

## Dependency and license review

Rust uses `Cargo.lock`, Studio uses `package-lock.json`, Python requirements are fully pinned, and
the base images and default model are digest/checksum pinned. CI builds and smoke-tests the image,
runs the Node production dependency audit, and scans the resulting OS/application image for
high/critical known vulnerabilities. Locally, equivalent checks are:

```sh
npm --prefix studio audit --omit=dev --audit-level=high
docker run --rm -v /var/run/docker.sock:/var/run/docker.sock \
  aquasec/trivy:0.67.2 image --exit-code 1 --ignore-unfixed \
  --severity HIGH,CRITICAL reel-maestro:local
```

The image includes components under their own terms: Reel Maestro (Apache-2.0), Node.js and npm
packages (their package metadata/licenses), Rust dependencies (`Cargo.lock` plus crate metadata),
FFmpeg and its Debian-linked libraries (LGPL/GPL depending on the packaged build), DejaVu fonts
(the DejaVu font license), Python packages including PyTorch and whisper-timestamped (their
package licenses), and OpenAI Whisper code/checkpoints (MIT). Frontend font license notices are
served under `/licenses/`. Before redistribution, generate an SBOM/license inventory and review
the actual image for the target platform; inclusion here is attribution, not a grant of model,
input, or output rights. Additional Whisper models and every hosted/H3 model remain subject to
their respective model cards, provider terms, and any separate license grant.
