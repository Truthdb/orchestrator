# orchestrator

Command-line tooling to administer the TruthDB organization.

## Commands

### `scripts/docker_repl.sh`

Starts a Docker-based TruthDB REPL using the sibling `truthdb` repo.

Example:

- `./scripts/docker_repl.sh`
- `./scripts/docker_repl.sh --ephemeral`
- `./scripts/docker_repl.sh --reset-data`
- `./scripts/docker_repl.sh --unconfined-seccomp`
- `./scripts/docker_repl.sh --confined-seccomp`

Notes:

- The script builds and runs the Docker image using the host Linux architecture by default:
  - `linux/amd64` on x86_64 hosts
  - `linux/arm64` on arm64 hosts
- On macOS, the script automatically uses `--security-opt seccomp=unconfined` for Docker Desktop `io_uring` support.
- Use `--confined-seccomp` to opt out of that behavior.
- It expects a sibling checkout at `../truthdb` unless `TRUTHDB_DIR` is set.

### `release-iso`

Creates and pushes the tags needed to build a matching installer ISO release, then waits until the expected release assets exist (and have stabilized) before proceeding to the next repo.

It tags the **local** clones and pushes tags to `origin`, so it behaves like doing the release manually in each repo.

Requirements:

- Local clones present under one directory:
	- `truthdb/`
	- `installer/`
	- `installer-kernel/`
	- `installer-iso/`
- Git auth configured for pushing tags (SSH keys or HTTPS credentials).
- `GITHUB_TRUTHDB_TOKEN` (or `GH_TOKEN`, or `GITHUB_TOKEN`) set for polling GitHub Releases.

Token setup (PAT):

Orchestrator uses the token only to *read* GitHub Releases/Assets while it waits. Tag pushing still uses your normal `git` credentials.

Option A: Fine-grained PAT (recommended)

Fine-grained tokens are scoped to a **resource owner** (your user *or* an organization). If you only see personal repos, you likely created the token under your user instead of the org, or the org policy disallows fine-grained PATs.

1. Go to GitHub → **Settings** → **Developer settings** → **Personal access tokens** → **Fine-grained tokens**.
2. Create a new token:
	- **Resource owner**: select `Truthdb` (or the org that owns the repos)
	- **Repository access**: select the needed repos (`installer-kernel`, `installer`, `installer-iso`, `truthdb`, `orchestrator`, `website`, `docs`, `.github`) *or* choose “All repositories” if you prefer
	- **Permissions (minimum)**:
		- **Metadata**: Read-only
		- **Contents**: Read-only (covers Releases/Assets API access)
		- **Actions**: Read-only (needed by `monitor` to read workflow run status)
3. If your org uses SSO, GitHub may require you to **authorize** the token for that org after creation.
4. Copy the token value (you won’t see it again).

Option B: Classic PAT

1. Go to GitHub → **Settings** → **Developer settings** → **Personal access tokens** → **Tokens (classic)**.
2. For *public repos*, a token with **no scopes** is usually sufficient (it’s still authenticated, so it avoids the very low unauthenticated API rate limit).
	- If you run into permission errors, add **public_repo**.
3. Copy the token value.

Set the token in your shell:

- One-shot:
	- `export GITHUB_TRUTHDB_TOKEN=...`
	- `./orchestrator release-iso --version v1.2.3`

- Inline for a single command:
	- `GITHUB_TRUTHDB_TOKEN=... ./orchestrator release-iso --version v1.2.3`

Notes:

- Orchestrator also accepts `GH_TOKEN` and `GITHUB_TOKEN` (same value). If multiple are set, it prefers `GITHUB_TRUTHDB_TOKEN`, then `GH_TOKEN`, then `GITHUB_TOKEN`.
- Prefer tokens with an expiration date; rotate if leaked.
- If `monitor` shows only unknown/blank rows, verify the token value itself is valid. A malformed token can make GitHub return auth failures for every repo.

Example:

- `GITHUB_TRUTHDB_TOKEN=... ./orchestrator release-iso --version v1.2.3`

Version format:

- `--version` must be SemVer.
- Accepted examples: `1.2.3`, `v1.2.3`, `1.2.3-rc.1`, `v1.2.3-rc.1`
- The `v` prefix is optional; orchestrator will normalize tags to `v{semver}`.

Resume example (if some tags/releases already exist):

- `GITHUB_TRUTHDB_TOKEN=... ./orchestrator release-iso --version v1.2.3 --resume`

Notes:

- Preflight safety checks are strict:
	- each repo must have a clean working tree
	- each repo must be on a branch (not detached)
	- each repo's `HEAD` must match `origin/<branch>`
	- the tag must not already exist locally or on `origin`

- `--resume` changes behavior:
	- if a repo already has the tag on `origin`, orchestrator skips creating/pushing the tag for that repo
	- it still polls GitHub Releases for required assets and continues to the next repo
	- for repos not yet tagged on `origin`, strict preflight still applies

### `monitor`

Shows a live TUI dashboard for the TruthDB organization.

Current behavior:

- Reads the latest `ci.yml` workflow run status for each repo's default branch
- Shows the latest release tag for each repo
- Shows how far the default branch is ahead of the latest release tag

Notes:

- `monitor` requires a TUI-capable terminal; `--no-tui` is not supported for this command
- For authenticated status, set `GITHUB_TRUTHDB_TOKEN`, `GH_TOKEN`, or `GITHUB_TOKEN`
- Fine-grained tokens need **Actions: Read-only** in addition to metadata/contents access

Example:

- `./orchestrator monitor`
- `GITHUB_TRUTHDB_TOKEN=... ./orchestrator monitor --poll-interval-secs 30`
