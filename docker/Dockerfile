# Self-contained multi-stage build for the Edgion federation Hub (Center).
#
# Unlike the Edgion monorepo's shared multi-binary Dockerfile, this builds the
# single edgion-center binary with the dashboard embedded (rust-embed), in one
# image, with no dependency on the monorepo's CI machinery.
#
# Build CONTEXT layout (produced by docker/build.sh — see that script):
#   <context>/EdgionCenter/                 the center source
#   <context>/Edgion/Cargo.toml             trimmed workspace root manifest
#   <context>/Edgion/edgion-resources/      the shared crate (path dependency)
#
# The relative layout mirrors local dev so the Cargo path dependency
# `edgion-resources = { path = "../Edgion/edgion-resources" }` resolves
# unchanged. (For a CI/registry build you may instead pin edgion-resources to a
# git rev and build with the EdgionCenter dir alone as context.)
#
# Usage: docker/build.sh   (do NOT `docker build .` directly — the context must
# include the shared crate; the script stages it.)
#
# Multi-arch: this Dockerfile is platform-agnostic. docker/build.sh drives
# `docker buildx build --platform linux/amd64,linux/arm64` to emit a single
# multi-arch manifest. The Rust stage compiles natively for its target platform
# (the non-host arch runs under QEMU/Rosetta emulation); BuildKit cache mounts,
# keyed per target arch, keep emulated rebuilds incremental.

ARG RUST_VERSION=1.92
ARG NODE_VERSION=20
ARG RUNTIME_BASE=debian:bookworm-slim

# ── Stage 1: build the dashboard (vite) ───────────────────────────────────────
# Pinned to the build platform: the dashboard output (web/dist) is static JS/CSS,
# identical across target arches, so building it once natively avoids compiling
# it under emulation for every target platform.
FROM --platform=$BUILDPLATFORM node:${NODE_VERSION}-bookworm AS web
WORKDIR /web
# Install deps against the lockfile first (better layer caching).
COPY EdgionCenter/web/package.json EdgionCenter/web/package-lock.json ./
RUN npm ci
COPY EdgionCenter/web/ ./
RUN npm run build
# → /web/dist

# ── Stage 2: build the Rust binary with the dashboard embedded ────────────────
FROM rust:${RUST_VERSION}-bookworm AS build
# Build deps: protoc (tonic-build/prost-build), plus cmake + libclang + pkg-config
# + libssl for native crates (aws-lc-sys via rustls's aws-lc-rs provider, etc.).
# lld: faster linker with lower peak memory than GNU ld — matters most for the
# non-host arch, which links under QEMU/Rosetta emulation.
RUN apt-get update && apt-get install -y --no-install-recommends \
    protobuf-compiler \
    cmake \
    libclang-dev \
    lld \
    pkg-config \
    libssl-dev \
    && rm -rf /var/lib/apt/lists/*
# Link with lld and strip symbols from the release binary (smaller image). Scoped
# to the image build via RUSTFLAGS so a local `cargo build --release` keeps its
# symbols for debugging.
ENV RUSTFLAGS="-C link-arg=-fuse-ld=lld -C strip=symbols"
# Shared crate first (path dependency target), then the center sources. The
# trimmed workspace root manifest must accompany the crate so its
# `*.workspace = true` inheritance resolves (edgion-resources is a workspace
# member and cannot be parsed without it).
COPY Edgion/Cargo.toml /src/Edgion/Cargo.toml
COPY Edgion/edgion-resources /src/Edgion/edgion-resources
WORKDIR /src/EdgionCenter
COPY EdgionCenter/Cargo.toml EdgionCenter/Cargo.lock EdgionCenter/build.rs ./
COPY EdgionCenter/proto ./proto
COPY EdgionCenter/src ./src
# Staged dashboard assets so rust-embed (`#[folder = "web/dist/"]`) finds them.
COPY --from=web /web/dist ./web/dist
# TARGETARCH (auto-provided by BuildKit) keys the per-arch target cache so the
# amd64 and arm64 builds don't clobber each other's artifacts. Cache mounts are
# ephemeral and never committed to the image layer, so the compiled binary is
# copied out to a real path for the runtime stage's COPY --from to pick up.
ARG TARGETARCH
# registry/git caches are shared across the parallel per-arch builds (cargo's own
# package-cache lock coordinates concurrent writes), so they are NOT `locked` —
# locking would serialize the amd64 and arm64 compiles for the whole RUN. Only
# the arch-specific target dir is locked, and keyed by TARGETARCH so the two
# arches keep separate artifacts.
RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/EdgionCenter/target,id=center-target-${TARGETARCH},sharing=locked \
    cargo build --release --features embed-dashboard \
    && cp target/release/edgion-center /usr/local/bin/edgion-center

# ── Stage 3: slim runtime ─────────────────────────────────────────────────────
FROM ${RUNTIME_BASE}
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 procps \
    && rm -rf /var/lib/apt/lists/*
# Non-root runtime user.
RUN groupadd -g 1000 edgion 2>/dev/null || groupadd edgion && \
    useradd -r -u 1000 -g edgion edgion 2>/dev/null || useradd -r -g edgion edgion
WORKDIR /usr/local/edgion
COPY --from=build /usr/local/bin/edgion-center /usr/local/bin/edgion-center
USER edgion
ENV RUST_LOG=info \
    RUST_BACKTRACE=1
# Informational: admin API / fed-sync gRPC / probe / metrics.
EXPOSE 12201 12251 12200 12290
ENTRYPOINT ["edgion-center"]
CMD ["-c", "/etc/edgion/edgion-center.yaml"]

LABEL org.opencontainers.image.title="edgion-center" \
      org.opencontainers.image.description="Edgion federation hub (Center): FederationSync server, admin API, embedded dashboard" \
      org.opencontainers.image.source="https://github.com/Pandaala/EdgionCenter" \
      org.opencontainers.image.licenses="Apache-2.0"
