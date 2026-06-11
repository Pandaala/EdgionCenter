#!/usr/bin/env bash
# Build the edgion-center Docker image (single binary, dashboard embedded).
#
# The image build needs the shared `edgion-resources` crate, which lives in the
# sibling Edgion repo and is referenced via a Cargo path dependency
# (`../Edgion/edgion-resources`). To keep that path resolvable inside the image
# AND keep the build context lean, this script stages a temporary context with
# the relative layout the Dockerfile expects:
#
#   <ctx>/EdgionCenter/                source (no target/node_modules/dist)
#   <ctx>/Edgion/edgion-resources/     the shared crate (no target)
#
# Usage:
#   docker/build.sh [-t IMAGE_TAG] [-r PATH_TO_EDGION_REPO]
# Defaults: IMAGE_TAG=edgion/edgion-center:dev, Edgion repo = ../Edgion
set -euo pipefail

IMAGE_TAG="edgion/edgion-center:dev"
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CENTER_DIR="$(cd "${SCRIPT_DIR}/.." && pwd)"
EDGION_DIR="$(cd "${CENTER_DIR}/.." && pwd)/Edgion"

while getopts ":t:r:" opt; do
    case "${opt}" in
        t) IMAGE_TAG="${OPTARG}" ;;
        r) EDGION_DIR="$(cd "${OPTARG}" && pwd)" ;;
        *) echo "usage: $0 [-t IMAGE_TAG] [-r PATH_TO_EDGION_REPO]" >&2; exit 2 ;;
    esac
done

RESOURCES_DIR="${EDGION_DIR}/edgion-resources"
if [[ ! -d "${RESOURCES_DIR}" ]]; then
    echo "error: edgion-resources not found at ${RESOURCES_DIR}" >&2
    echo "       pass the Edgion repo path with -r <path>." >&2
    exit 1
fi

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

echo "Building ${IMAGE_TAG} ..."
docker build -f "${CENTER_DIR}/docker/Dockerfile" -t "${IMAGE_TAG}" "${CTX}"
echo "Built ${IMAGE_TAG}"
