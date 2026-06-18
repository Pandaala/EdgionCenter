#!/usr/bin/env bash
# Build the edgion-center Docker image (single binary, dashboard embedded),
# with optional multi-architecture (linux/amd64 + linux/arm64) support.
#
# The image build needs the shared `edgion-resources` crate, which lives in the
# sibling Edgion repo and is referenced via a Cargo path dependency
# (`../Edgion/edgion-resources`). To keep that path resolvable inside the image
# AND keep the build context lean, this script stages a temporary context with
# the relative layout the Dockerfile expects:
#
#   <ctx>/EdgionCenter/                source (no target/node_modules/dist)
#   <ctx>/Edgion/Cargo.toml            trimmed workspace root (members = resources)
#   <ctx>/Edgion/edgion-resources/     the shared crate (no target)
#
# `edgion-resources` is now an Edgion workspace member: its Cargo.toml inherits
# `edition`/`version`/deps via `*.workspace = true`, so it cannot be built in
# isolation. We stage a trimmed copy of the workspace root manifest (only
# `edgion-resources` as a member) so that inheritance resolves without pulling in
# the other members (notably the ~56k-line `edgion-tests`).
#
# Multi-arch is produced with `docker buildx build --platform ...`, which emits a
# single manifest list covering every requested arch. The non-host arch compiles
# under QEMU/Rosetta emulation; the Dockerfile's per-arch BuildKit cache mounts
# keep emulated rebuilds incremental. A multi-platform image CANNOT be loaded
# into the local Docker image store (`--load` is single-platform only), so
# building more than one arch requires --push to a registry.
#
# Usage:
#   cicd/build-image.sh                                          # host arch, loaded locally
#   cicd/build-image.sh --arch amd64,arm64 --push                # multi-arch -> registry
#   cicd/build-image.sh --platform linux/amd64,linux/arm64 --push
#   cicd/build-image.sh -t pandaala/edgion-center:dev -r ../Edgion
#
# Options:
#   -t IMAGE_TAG     Full image tag, overrides the assembled default. Repeatable.
#   --version VER    Version component of the default tag (overrides the git tag)
#   -r EDGION_DIR    Path to the Edgion repo (default: ../Edgion)
#   --arch LIST      Comma-separated arches: amd64,arm64 (mapped to linux/<arch>)
#   --platform LIST  Comma-separated buildx platforms (e.g. linux/amd64,linux/arm64)
#   --push           Push the result to the registry (required for >1 platform)
#   --load           Load the result into the local image store (single platform only)
#   -h, --help       Show this help
set -euo pipefail

# ============================================================================
# Configuration  (edit these to customise the build)
# ============================================================================
# Fallback version used only when HEAD has no git tag. Normal flow: tag the
# commit (e.g. `git tag v0.3.2`) and the script picks it up automatically via
# `git describe --tags --exact-match`. Override per-run with --version / VERSION.
DEFAULT_VERSION="0.3.2"
# Default tag is assembled as ${IMAGE_REGISTRY}/${IMAGE_NAMESPACE}/${IMAGE_NAME}:${VERSION}
# matching the Edgion repo's convention (docker.io/pandaala/edgion-*).
IMAGE_REGISTRY="${IMAGE_REGISTRY:-docker.io}"
IMAGE_NAMESPACE="${IMAGE_NAMESPACE:-pandaala}"
IMAGE_NAME="${IMAGE_NAME:-edgion-center}"
# ============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CENTER_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
EDGION_DIR="$(cd "${CENTER_DIR}/.." && pwd)/Edgion"

# Version precedence: --version flag / VERSION env > current git tag > DEFAULT_VERSION.
# Resolved after arg parsing; empty here means "auto-detect from git tag".
VERSION="${VERSION:-}"

IMAGE_TAGS=()
PLATFORMS=""
PUSH=false
LOAD=false
BUILDER_NAME="edgion-center-builder"

host_platform() {
    case "$(uname -m)" in
        x86_64|amd64) echo "linux/amd64" ;;
        arm64|aarch64) echo "linux/arm64" ;;
        *) echo "linux/amd64" ;;
    esac
}

# Map "amd64,arm64" -> "linux/amd64,linux/arm64"; pass linux/* through unchanged.
arch_list_to_platforms() {
    local out=() item
    IFS=',' read -ra items <<< "$1"
    for item in "${items[@]}"; do
        case "${item}" in
            linux/*) out+=("${item}") ;;
            amd64|x86_64) out+=("linux/amd64") ;;
            arm64|aarch64) out+=("linux/arm64") ;;
            *) echo "error: unknown arch '${item}' (use amd64, arm64, or linux/<arch>)" >&2; exit 2 ;;
        esac
    done
    local IFS=,
    echo "${out[*]}"
}

usage() {
    cat <<EOF
Build the edgion-center Docker image (single binary, dashboard embedded),
optionally for multiple architectures (linux/amd64 + linux/arm64).

Usage:
  cicd/build-image.sh                                          # host arch, loaded locally
  cicd/build-image.sh --arch amd64,arm64 --push                # multi-arch -> registry
  cicd/build-image.sh --platform linux/amd64,linux/arm64 --push
  cicd/build-image.sh -t pandaala/edgion-center:dev -r ../Edgion

Options:
  -t IMAGE_TAG     Full image tag, overrides the assembled default. Repeatable.
  --version VER    Version component of the default tag (overrides the git tag)
  -r EDGION_DIR    Path to the Edgion repo (default: ../Edgion)
  --arch LIST      Comma-separated arches: amd64,arm64 (mapped to linux/<arch>)
  --platform LIST  Comma-separated buildx platforms (e.g. linux/amd64,linux/arm64)
  --push           Push the result to the registry (required for >1 platform)
  --load           Load the result into the local image store (single platform only)
  -h, --help       Show this help

Default tag: ${IMAGE_REGISTRY}/${IMAGE_NAMESPACE}/${IMAGE_NAME}:<version>
  Version resolution: --version / VERSION env > current git tag > DEFAULT_VERSION
  (${DEFAULT_VERSION}). Override the other pieces via env: IMAGE_REGISTRY
  (docker.io), IMAGE_NAMESPACE (pandaala), IMAGE_NAME (edgion-center).

Note: a multi-platform image cannot be loaded into the local Docker image store
(--load is single-platform only), so building more than one arch requires --push.
EOF
}

while [[ $# -gt 0 ]]; do
    case "$1" in
        -t) IMAGE_TAGS+=("$2"); shift 2 ;;
        --version) VERSION="$2"; shift 2 ;;
        -r) EDGION_DIR="$(cd "$2" && pwd)"; shift 2 ;;
        --arch) PLATFORMS="$(arch_list_to_platforms "$2")"; shift 2 ;;
        --platform) PLATFORMS="$(arch_list_to_platforms "$2")"; shift 2 ;;
        --push) PUSH=true; shift ;;
        --load) LOAD=true; shift ;;
        -h|--help) usage; exit 0 ;;
        *) echo "error: unknown option '$1'" >&2; usage >&2; exit 2 ;;
    esac
done

# Resolve version: explicit --version/VERSION wins; otherwise use the current
# git tag (e.g. v0.3.2); otherwise fall back to DEFAULT_VERSION.
if [[ -z "${VERSION}" ]]; then
    GIT_TAG=""
    if git -C "${CENTER_DIR}" rev-parse --git-dir >/dev/null 2>&1; then
        GIT_TAG="$(git -C "${CENTER_DIR}" describe --tags --exact-match 2>/dev/null || echo "")"
    fi
    VERSION="${GIT_TAG:-${DEFAULT_VERSION}}"
fi

# Defaults.
[[ ${#IMAGE_TAGS[@]} -eq 0 ]] && IMAGE_TAGS=("${IMAGE_REGISTRY}/${IMAGE_NAMESPACE}/${IMAGE_NAME}:${VERSION}")
[[ -z "${PLATFORMS}" ]] && PLATFORMS="$(host_platform)"

# Decide output mode and validate against the platform count.
PLATFORM_COUNT="$(awk -F, '{print NF}' <<< "${PLATFORMS}")"
if [[ "${PUSH}" == "true" && "${LOAD}" == "true" ]]; then
    echo "error: --push and --load are mutually exclusive" >&2
    exit 2
fi
if [[ "${PUSH}" != "true" && "${LOAD}" != "true" ]]; then
    # Default: load locally for a single platform; for multi-arch, --push is required.
    if [[ "${PLATFORM_COUNT}" -gt 1 ]]; then
        echo "error: building ${PLATFORM_COUNT} platforms (${PLATFORMS}) cannot be loaded locally." >&2
        echo "       A multi-platform image must be pushed to a registry. Re-run with --push," >&2
        echo "       or pick a single arch (e.g. --arch arm64) for a local --load build." >&2
        exit 2
    fi
    LOAD=true
elif [[ "${LOAD}" == "true" && "${PLATFORM_COUNT}" -gt 1 ]]; then
    echo "error: --load supports a single platform only (got ${PLATFORMS}). Use --push." >&2
    exit 2
fi

RESOURCES_DIR="${EDGION_DIR}/edgion-resources"
if [[ ! -d "${RESOURCES_DIR}" ]]; then
    echo "error: edgion-resources not found at ${RESOURCES_DIR}" >&2
    echo "       pass the Edgion repo path with -r <path>." >&2
    exit 1
fi

# Ensure a buildx builder exists (the default 'docker' driver cannot do
# multi-platform builds nor --push of a manifest list).
ensure_buildx() {
    if ! docker buildx version >/dev/null 2>&1; then
        echo "error: docker buildx not available. Install Buildx or upgrade Docker." >&2
        exit 1
    fi
    if ! docker buildx inspect "${BUILDER_NAME}" >/dev/null 2>&1; then
        echo "Creating buildx builder '${BUILDER_NAME}' ..."
        docker buildx create --name "${BUILDER_NAME}" --driver docker-container --bootstrap >/dev/null
    fi
}
ensure_buildx

CTX="$(mktemp -d)"
trap 'rm -rf "${CTX}"' EXIT

echo "Staging build context at ${CTX} ..."
mkdir -p "${CTX}/EdgionCenter" "${CTX}/Edgion"
# Center source: exclude build artifacts and node deps; the dashboard is rebuilt
# in the image's web stage.
rsync -a \
    --exclude '/target' \
    --exclude '/web/node_modules' \
    --exclude '/web/dist' \
    --exclude '/.git' \
    "${CENTER_DIR}/" "${CTX}/EdgionCenter/"
# Shared crate (exclude its build artifacts).
rsync -a --exclude '/target' --exclude '/.git' "${RESOURCES_DIR}/" "${CTX}/Edgion/edgion-resources/"
# Trimmed workspace root: keep [workspace.package] + [workspace.dependencies]
# (the source of truth for edgion-resources' inherited fields) but list only
# edgion-resources as a member so the other crate dirs need not be staged.
sed -E \
    -e 's/^members = .*/members = ["edgion-resources"]/' \
    -e 's/^default-members = .*/default-members = ["edgion-resources"]/' \
    "${EDGION_DIR}/Cargo.toml" > "${CTX}/Edgion/Cargo.toml"

# Assemble the buildx invocation.
BUILD_CMD=(docker buildx build --builder "${BUILDER_NAME}" --platform "${PLATFORMS}" -f "${CENTER_DIR}/cicd/docker/Dockerfile")
for tag in "${IMAGE_TAGS[@]}"; do
    BUILD_CMD+=(-t "${tag}")
done
if [[ "${PUSH}" == "true" ]]; then
    BUILD_CMD+=(--push)
else
    BUILD_CMD+=(--load)
fi
BUILD_CMD+=("${CTX}")

echo "Building ${IMAGE_TAGS[*]} for ${PLATFORMS} ($([[ "${PUSH}" == "true" ]] && echo push || echo load)) ..."
"${BUILD_CMD[@]}"
echo "Done: ${IMAGE_TAGS[*]} (${PLATFORMS})"
