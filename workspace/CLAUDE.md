# CLAUDE.md — TruthDB workspace

This file lives in `orchestrator/workspace/` and is synced to the workspace root by `orchestrator workspace-update`. It is the canonical source of build and development rules for agents and developers.

## Workspace layout

This is a multi-repo workspace. Each subdirectory is a separate Git repo:

| Directory                        | What it is                                      | Language   |
|----------------------------------|------------------------------------------------|------------|
| `truthdb/`                       | Core database service + CLI + bench tool        | Rust       |
| `orchestrator/`                  | Developer/admin CLI (release automation, TUI)   | Rust       |
| `installer/`                     | Bare-metal installer (runs in initramfs)         | Rust       |
| `installer-iso/`                | Bootable UEFI installer ISO builder              | Shell      |
| `installer-kernel/`             | Custom Linux kernel config for installer         | Kconfig    |
| `installer-kernel-builder-image/`| Docker image used to build the kernel           | Dockerfile |
| `website/`                       | Public website                                   | TypeScript |
| `docs/`                          | Architecture, specs, feature plans               | Markdown   |
| `.github/`                       | Org-level GitHub templates                       | Markdown   |

## Critical build rules

All Rust crates use `edition = "2024"` (requires Rust 1.85+). CI uses `rust:1.92-bookworm`.

### truthdb/ (the database — all crates)

**Cannot build on macOS. Ever.** The `io-uring` and `libc` dependencies require Linux headers. The three binary crates (`truthdb`, `truthdb-cli`, `truthdb-bench`) enforce this with `compile_error!` on non-Linux targets. Even `cargo check` fails on macOS.

| Platform         | How to build / check                                                            |
|------------------|--------------------------------------------------------------------------------|
| **macOS**        | `docker run --rm -v "$PWD/truthdb":/src -w /src rust:1-bookworm cargo check`   |
| **Linux native** | `cd truthdb && cargo check` (or `cargo build --release`)                        |
| **WSL2**         | `cd truthdb && cargo check` (native build works, WSL2 kernel has io_uring)      |

To run tests (requires io_uring at runtime):

| Platform         | How to test                                                                                      |
|------------------|--------------------------------------------------------------------------------------------------|
| **macOS**        | `docker run --rm --security-opt seccomp=unconfined -v "$PWD/truthdb":/src -w /src rust:1-bookworm cargo test --workspace` |
| **Linux native** | `cd truthdb && cargo test --workspace`                                                           |
| **WSL2**         | `cd truthdb && cargo test --workspace` (if io_uring is available — see WSL2 notes below)          |

Lint:
- `cargo fmt -- --check`
- `cargo clippy --all-targets --all-features -- -D warnings`

Same Docker wrapper applies on macOS for fmt/clippy.

### orchestrator/

**Builds natively on all platforms.** No Linux-only dependencies.

```sh
cd orchestrator && cargo build
cd orchestrator && cargo check
cd orchestrator && cargo test
```

### installer/

**Linux-only.** Targets `x86_64-unknown-linux-musl` via `.cargo/config.toml`. Uses Linux-specific APIs (`/sys/block`, `sfdisk`, `mount`, `chroot`). No `compile_error!` guard but will not produce a useful binary on non-Linux.

| Platform         | How to build                                                                                                    |
|------------------|-----------------------------------------------------------------------------------------------------------------|
| **macOS**        | `docker run --rm -v "$PWD/installer":/src -w /src rust:1-bookworm sh -c "rustup target add x86_64-unknown-linux-musl && cargo build --release"` |
| **Linux native** | `cd installer && rustup target add x86_64-unknown-linux-musl && cargo build --release --target x86_64-unknown-linux-musl` |
| **WSL2**         | Same as Linux native                                                                                             |

### installer-iso/

Container-based builds only. The scripts handle everything:

```sh
cd installer-iso && ./build_in_container.sh                     # dev mode (local artifacts)
cd installer-iso && INPUT_MODE=release ./build_in_container.sh  # release-like (published artifacts)
```

### installer-kernel/

Container-based builds only:

```sh
cd installer-kernel && ./build_in_container.sh
```

To build kernel + ISO together:

```sh
cd installer-kernel && ./build_iso_with_local_kernel.sh
```

### website/

**Builds natively on all platforms.**

```sh
cd website && npm install && npm run build
cd website && npm run lint
cd website && npm run dev   # dev server
```

## Docker REPL and benchmarks

Interactive REPL (starts server + CLI in one container):

```sh
orchestrator/scripts/docker_repl.sh
```

Benchmark (starts server + runs truthdb-bench):

```sh
orchestrator/scripts/docker_bench.sh
orchestrator/scripts/docker_bench.sh -- --operations 5000 --connections 4
```

Both scripts:
- Auto-detect host architecture (`linux/amd64` or `linux/arm64`)
- Auto-enable `seccomp=unconfined` on macOS (required for io_uring in Docker Desktop)
- Build the Docker image automatically if missing
- Use `--rebuild` to force a fresh image build

## macOS-specific rules

1. **Never run `cargo check`, `cargo build`, or `cargo test` directly for `truthdb/` or `installer/`.** Always use Docker.
2. Docker Desktop requires `--security-opt seccomp=unconfined` for anything that touches io_uring at runtime (tests, REPL, bench). The launcher scripts handle this automatically.
3. The orchestrator and website are the only components that build and run natively on macOS.

## Linux-specific notes

On native Linux (bare metal or VM), everything builds and runs directly:

- `truthdb/`: `cargo build --release` and `cargo test --workspace` work natively.
- io_uring requires kernel 5.1+ (practically 5.10+ for the features used here). Most modern distros satisfy this.
- Tests do not need `seccomp=unconfined` when running natively (that is only a Docker/container restriction).
- If running tests inside a container on Linux, you still need `--security-opt seccomp=unconfined` (same as CI does).

## WSL2-specific notes

WSL2 generally works like native Linux with caveats:

- **Build**: `cargo check` and `cargo build` work natively inside WSL2.
- **io_uring support**: WSL2 kernel 5.10+ supports io_uring. Check with `uname -r`. Microsoft ships 5.15+ by default on current Windows builds. If `io_uring_setup` returns `ENOSYS`, your WSL kernel is too old — update via `wsl --update`.
- **Docker Desktop for Windows (WSL2 backend)**: Needs `--security-opt seccomp=unconfined` just like macOS. The launcher scripts auto-detect macOS but not WSL2-with-Docker-Desktop — pass `--unconfined-seccomp` explicitly if using Docker inside WSL2.
- **Docker running natively inside WSL2** (not Docker Desktop): Same as Linux — seccomp may block io_uring depending on the default profile. Use `--security-opt seccomp=unconfined` if you see `EPERM` errors.

## Quick reference: "I want to check my changes compile"

| I changed...               | Run this                                                                                   |
|----------------------------|--------------------------------------------------------------------------------------------|
| `truthdb/` code (on macOS) | `docker run --rm -v "$PWD/truthdb":/src -w /src rust:1-bookworm cargo check`              |
| `truthdb/` code (on Linux/WSL2) | `cd truthdb && cargo check`                                                           |
| `orchestrator/` code       | `cd orchestrator && cargo check`                                                           |
| `installer/` code (on macOS) | `docker run --rm -v "$PWD/installer":/src -w /src rust:1-bookworm cargo check`           |
| `installer/` code (on Linux/WSL2) | `cd installer && cargo check`                                                       |
| `website/` code            | `cd website && npm run build`                                                              |

## CI

Each repo has its own `.github/workflows/ci.yml`. The `truthdb/` CI runs in `rust:1.92-bookworm` containers with `--security-opt seccomp=unconfined` for tests.

## Specs and design docs

- Storage design: `docs/development/specs/STORAGE.md`
- WAL requirements: `docs/development/specs/WAL.md`
- Architecture overview: `docs/development/architecture/OVERVIEW.md`
- Start here (org overview): `docs/development/specs/TRUTHDB-START-HERE.md`
- Feature plans: `docs/features/`
- Capability catalogue: `docs/features/truthdb_capability_catalogue.md`

## Port and protocol

TruthDB listens on port **9623** (TCP). Custom binary protocol using bincode serialization with 8-byte frame headers. Protocol version 1.
