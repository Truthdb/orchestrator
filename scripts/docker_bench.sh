#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ORCHESTRATOR_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
WORKSPACE_ROOT="$(cd "${ORCHESTRATOR_ROOT}/.." && pwd)"
TRUTHDB_DIR="${TRUTHDB_DIR:-${WORKSPACE_ROOT}/truthdb}"

IMAGE_TAG="${IMAGE_TAG:-truthdb-bench:local}"
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
Usage: ./scripts/docker_bench.sh [OPTIONS] [-- BENCH_ARGS...]

Builds and runs the TruthDB benchmark inside Docker. The container starts the
TruthDB server, waits for it to be ready, then runs truthdb-bench against it.

Options:
  --rebuild               Rebuild the Docker image before launch
  --unconfined-seccomp    Run with --security-opt seccomp=unconfined (default on macOS)
  --confined-seccomp      Do not pass --security-opt seccomp=unconfined
  --help                  Show this help text

Benchmark arguments (after --):
  --operations N          Total operations per phase (default: 100000)
  --connections N         Number of concurrent TCP connections (default: 1)
  --payload-size SIZE     Document size: small, medium, large (default: medium)
  --write-only            Skip the read phase
  --read-only             Skip the write phase

Examples:
  ./scripts/docker_bench.sh
  ./scripts/docker_bench.sh --rebuild
  ./scripts/docker_bench.sh -- --operations 5000 --connections 4
  ./scripts/docker_bench.sh -- --operations 1000 --write-only --payload-size large

Environment:
  TRUTHDB_DIR      Path to the truthdb repo (default: sibling repo at ../truthdb)
  IMAGE_TAG        Docker image tag to use/build (default: truthdb-bench:local)
  IMAGE_PLATFORM   Docker platform to build/run (default: host Linux arch)
EOF
}

BENCH_ARGS=()
while [[ $# -gt 0 ]]; do
  case "$1" in
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
    --)
      shift
      BENCH_ARGS=("$@")
      break
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

if [[ ! -f "${TRUTHDB_DIR}/Dockerfile.bench" ]]; then
  echo "ERROR: Dockerfile.bench not found in ${TRUTHDB_DIR}" >&2
  exit 1
fi

if [[ "${HOST_OS}" = "Darwin" && "${UNCONFINED_SECCOMP}" = "1" ]]; then
  echo "INFO: enabling seccomp=unconfined for Docker Desktop io_uring support" >&2
fi

build_image() {
  docker build \
    --platform "${IMAGE_PLATFORM}" \
    -t "${IMAGE_TAG}" \
    -f "${TRUTHDB_DIR}/Dockerfile.bench" \
    "${TRUTHDB_DIR}"
}

if [[ "${REBUILD_IMAGE}" = "1" ]]; then
  build_image
elif ! docker image inspect "${IMAGE_TAG}" >/dev/null 2>&1; then
  build_image
fi

docker_args=(run --rm -e "STATE_DIRECTORY=/data")

if [[ "${UNCONFINED_SECCOMP}" = "1" ]]; then
  docker_args+=(--security-opt seccomp=unconfined)
fi

docker "${docker_args[@]}" --platform "${IMAGE_PLATFORM}" "${IMAGE_TAG}" "${BENCH_ARGS[@]+"${BENCH_ARGS[@]}"}"
