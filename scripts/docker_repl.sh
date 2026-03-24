#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ORCHESTRATOR_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
WORKSPACE_ROOT="$(cd "${ORCHESTRATOR_ROOT}/.." && pwd)"
TRUTHDB_DIR="${TRUTHDB_DIR:-${WORKSPACE_ROOT}/truthdb}"

IMAGE_TAG="${IMAGE_TAG:-truthdb-repl:local}"
VOLUME_NAME="${VOLUME_NAME:-truthdb-repl-data}"
PERSIST_MODE="persist"
RESET_DATA=0
REBUILD_IMAGE=0
UNCONFINED_SECCOMP=0
HOST_OS="$(uname -s)"

default_image_platform() {
  case "$(uname -m)" in
    x86_64|amd64)
      printf '%s\n' "linux/amd64"
      ;;
    arm64|aarch64)
      printf '%s\n' "linux/arm64"
      ;;
    *)
      echo "ERROR: unsupported host architecture: $(uname -m)" >&2
      echo "Set IMAGE_PLATFORM explicitly to linux/amd64 or linux/arm64." >&2
      exit 1
      ;;
  esac
}

IMAGE_PLATFORM="${IMAGE_PLATFORM:-$(default_image_platform)}"

if [[ "${HOST_OS}" = "Darwin" ]]; then
  UNCONFINED_SECCOMP=1
fi

usage() {
  cat <<'EOF'
Usage: ./scripts/docker_repl.sh [--persist | --ephemeral] [--reset-data] [--rebuild]

Starts a Docker container that runs the TruthDB server in the background and
opens truthdb-cli in REPL mode against it.

Options:
  --persist     Use the named Docker volume for database state (default)
  --ephemeral   Use disposable container-local storage
  --reset-data  Remove the persistent named volume before launch
  --rebuild     Rebuild the Docker image before launch
  --unconfined-seccomp
                Run the container with --security-opt seccomp=unconfined
  --confined-seccomp
                Do not pass --security-opt seccomp=unconfined
  --help        Show this help text

Environment:
  TRUTHDB_DIR    Path to the truthdb repo (default: sibling repo at ../truthdb)
  IMAGE_TAG      Docker image tag to use/build (default: truthdb-repl:local)
  IMAGE_PLATFORM Docker platform to build/run (default: host Linux arch, arm64 or amd64)
  VOLUME_NAME    Docker volume name for persisted state (default: truthdb-repl-data)
EOF
}

while [[ $# -gt 0 ]]; do
  case "$1" in
    --persist)
      PERSIST_MODE="persist"
      ;;
    --ephemeral)
      PERSIST_MODE="ephemeral"
      ;;
    --reset-data)
      RESET_DATA=1
      ;;
    --rebuild)
      REBUILD_IMAGE=1
      ;;
    --unconfined-seccomp)
      UNCONFINED_SECCOMP=1
      ;;
    --confined-seccomp)
      UNCONFINED_SECCOMP=0
      ;;
    --help|-h)
      usage
      exit 0
      ;;
    *)
      echo "ERROR: unknown argument: $1" >&2
      echo >&2
      usage >&2
      exit 1
      ;;
  esac
  shift
done

if [[ ! -d "${TRUTHDB_DIR}" ]]; then
  echo "ERROR: truthdb repo not found at ${TRUTHDB_DIR}" >&2
  exit 1
fi

if [[ ! -f "${TRUTHDB_DIR}/Dockerfile.repl" ]]; then
  echo "ERROR: Docker REPL assets not found in ${TRUTHDB_DIR}" >&2
  exit 1
fi

if [[ "${HOST_OS}" = "Darwin" && "${UNCONFINED_SECCOMP}" = "1" ]]; then
  echo "INFO: enabling seccomp=unconfined for Docker Desktop io_uring support" >&2
fi

if [[ "${PERSIST_MODE}" = "ephemeral" && "${RESET_DATA}" = "1" ]]; then
  echo "ERROR: --reset-data cannot be used with --ephemeral" >&2
  exit 1
fi

build_image() {
  docker build \
    --platform "${IMAGE_PLATFORM}" \
    -t "${IMAGE_TAG}" \
    -f "${TRUTHDB_DIR}/Dockerfile.repl" \
    "${TRUTHDB_DIR}"
}

if [[ "${REBUILD_IMAGE}" = "1" ]]; then
  build_image
elif ! docker image inspect "${IMAGE_TAG}" >/dev/null 2>&1; then
  build_image
fi

if [[ "${RESET_DATA}" = "1" ]]; then
  docker volume rm -f "${VOLUME_NAME}" >/dev/null 2>&1 || true
fi

docker_args=(run --rm --init -e "STATE_DIRECTORY=/data")

if [[ -t 0 && -t 1 ]]; then
  docker_args+=(-it)
else
  docker_args+=(-i)
fi

if [[ "${PERSIST_MODE}" = "persist" ]]; then
  docker_args+=(-v "${VOLUME_NAME}:/data")
fi

if [[ "${UNCONFINED_SECCOMP}" = "1" ]]; then
  docker_args+=(--security-opt seccomp=unconfined)
fi

docker "${docker_args[@]}" --platform "${IMAGE_PLATFORM}" "${IMAGE_TAG}"
