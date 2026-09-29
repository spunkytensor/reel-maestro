# syntax=docker/dockerfile:1.7

FROM rust:1.88.0-bookworm@sha256:af306cfa71d987911a781c37b59d7d67d934f49684058f96cf72079c3626bfe0 AS rust-build
WORKDIR /build
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --locked

FROM node:22-bookworm-slim@sha256:83f487e0a63425e5b4d146fb5e5be574bcbe1b7b843d3ebafdd95eaf7767a7e5 AS studio-build
WORKDIR /build/studio
COPY studio/package.json studio/package-lock.json ./
RUN npm ci
COPY studio/ ./
COPY docs/design/src/studio.css /build/docs/design/src/studio.css
COPY docs/design/src/assets/fonts/ /build/docs/design/src/assets/fonts/
RUN npm run build \
    && ./node_modules/.bin/esbuild server/index.ts \
      --bundle --platform=node --format=esm --packages=external \
      --outfile=server-dist/index.js \
    && npm prune --omit=dev

FROM python:3.11-slim-bookworm@sha256:528257d48c1da0dcecc2e725d1ae34498d60c965f1241e39cd6a85a8859bdf84 AS whisper-build
ENV VIRTUAL_ENV=/opt/whisper
RUN python -m venv "$VIRTUAL_ENV"
COPY docker/requirements-whisper.txt /tmp/requirements-whisper.txt
RUN "$VIRTUAL_ENV/bin/pip" install --disable-pip-version-check --no-cache-dir \
      pip==24.0 setuptools==80.9.0 wheel==0.45.1 \
    && "$VIRTUAL_ENV/bin/pip" install --disable-pip-version-check --no-cache-dir \
      --no-build-isolation \
      --extra-index-url https://download.pytorch.org/whl/cpu \
      -r /tmp/requirements-whisper.txt \
    && "$VIRTUAL_ENV/bin/pip" check \
    && "$VIRTUAL_ENV/bin/pip" uninstall --yes pip setuptools wheel

FROM node:22-bookworm-slim@sha256:83f487e0a63425e5b4d146fb5e5be574bcbe1b7b843d3ebafdd95eaf7767a7e5 AS runtime

ARG REELMAESTRO_UID=1000
ARG REELMAESTRO_GID=1000
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
      ca-certificates \
      ffmpeg \
      fonts-dejavu-core \
      python3 \
    && rm -rf /var/lib/apt/lists/* \
    && rm -rf /usr/local/lib/node_modules/npm /usr/local/bin/npm /usr/local/bin/npx \
    && test "$REELMAESTRO_UID" -ne 0 \
    && test "$REELMAESTRO_GID" -ne 0 \
    && userdel --remove node \
    && groupadd --non-unique --gid "$REELMAESTRO_GID" reelmaestro \
    && useradd --non-unique --uid "$REELMAESTRO_UID" --gid "$REELMAESTRO_GID" \
      --create-home --home-dir /home/reelmaestro reelmaestro \
    && ln -s /usr/bin/python3 /usr/local/bin/python \
    && mkdir -p /data/out /data/state /home/reelmaestro/.cache/whisper \
    && chown -R reelmaestro:reelmaestro /data /home/reelmaestro/.cache

ENV PATH=/opt/whisper/bin:/usr/local/bin:/usr/bin:/bin \
    HOME=/home/reelmaestro \
    LANG=C.UTF-8 \
    PYTHONDONTWRITEBYTECODE=1 \
    REELMAESTRO_HOST=0.0.0.0 \
    REELMAESTRO_PORT=3000 \
    REELMAESTRO_OUT_DIR=/data/out \
    REELMAESTRO_STATE_DIR=/data/state \
    REELMAESTRO_BINARY=/usr/local/bin/reelmaestro \
    REELMAESTRO_WHISPER_CMD=whisper_timestamped \
    REELMAESTRO_WHISPER_MODEL=base

WORKDIR /app
COPY --from=rust-build /build/target/release/reelmaestro /usr/local/bin/reelmaestro
COPY --from=whisper-build /opt/whisper /opt/whisper
COPY --from=studio-build /build/studio/node_modules ./studio/node_modules
COPY --from=studio-build /build/studio/server-dist ./studio/server
COPY --from=studio-build /build/studio/web/dist ./studio/web/dist
COPY LICENSE ./LICENSE
COPY docker/healthcheck.js /usr/local/lib/reelmaestro/healthcheck.js
COPY --chmod=0755 docker/offline-fixture.sh /usr/local/lib/reelmaestro/offline-fixture.sh

# OpenAI Whisper base model, pinned to the checksum encoded in the upstream model URL.
ADD --chown=reelmaestro:reelmaestro --chmod=0644 \
  --checksum=sha256:ed3a0b6b1c0edf879ad9b11b1af5a0e6ab5db9205f891f668f8b0e6c6326e34e \
  https://openaipublic.azureedge.net/main/whisper/models/ed3a0b6b1c0edf879ad9b11b1af5a0e6ab5db9205f891f668f8b0e6c6326e34e/base.pt \
  /home/reelmaestro/.cache/whisper/base.pt

USER reelmaestro:reelmaestro
WORKDIR /data/out
VOLUME ["/data/state", "/home/reelmaestro/.cache/whisper"]
EXPOSE 3000
STOPSIGNAL SIGTERM
HEALTHCHECK --interval=30s --timeout=10s --start-period=15s --retries=3 \
  CMD ["node", "/usr/local/lib/reelmaestro/healthcheck.js"]
CMD ["node", "/app/studio/server/index.js"]
