# orchestrator

Command-line tooling to administer the TruthDB organization.

## Commands

### `release-iso`

Creates and pushes the tags needed to build a matching installer ISO release, then waits until the expected release assets exist (and have stabilized) before proceeding to the next repo.

It tags the **local** clones and pushes tags to `origin`, so it behaves like doing the release manually in each repo.

Requirements:

- Local clones present under one directory:
	- `truthdb/`
	- `truthdb-cli/`
	- `truthdb-net/`
	- `truthdb-proto/`
	- `installer/`
	- `installer-kernel/`
	- `installer-iso/`
- Git auth configured for pushing tags (SSH keys or HTTPS credentials).
- `GITHUB_TOKEN` (or `GH_TOKEN`) set for polling GitHub Releases.

Token setup (PAT):

Orchestrator uses the token only to *read* GitHub Releases/Assets while it waits. Tag pushing still uses your normal `git` credentials.

Option A: Fine-grained PAT (recommended)

Fine-grained tokens are scoped to a **resource owner** (your user *or* an organization). If you only see personal repos, you likely created the token under your user instead of the org, or the org policy disallows fine-grained PATs.

1. Go to GitHub → **Settings** → **Developer settings** → **Personal access tokens** → **Fine-grained tokens**.
2. Create a new token:
	- **Resource owner**: select `Truthdb` (or the org that owns the repos)
	- **Repository access**: select the needed repos (`installer-kernel`, `installer`, `installer-iso`, `truthdb`, `truthdb-cli`, `truthdb-net`, `truthdb-proto`) *or* choose “All repositories” if you prefer
	- **Permissions (minimum)**:
		- **Metadata**: Read-only
		- **Contents**: Read-only (covers Releases/Assets API access)
3. If your org uses SSO, GitHub may require you to **authorize** the token for that org after creation.
4. Copy the token value (you won’t see it again).

Option B: Classic PAT

1. Go to GitHub → **Settings** → **Developer settings** → **Personal access tokens** → **Tokens (classic)**.
2. For *public repos*, a token with **no scopes** is usually sufficient (it’s still authenticated, so it avoids the very low unauthenticated API rate limit).
	- If you run into permission errors, add **public_repo**.
3. Copy the token value.

Set the token in your shell:

- One-shot:
	- `export GITHUB_TOKEN=...`
	- `./orchestrator release-iso --version v1.2.3`

- Inline for a single command:
	- `GITHUB_TOKEN=... ./orchestrator release-iso --version v1.2.3`

Notes:

- Orchestrator also accepts `GH_TOKEN` (same value). If both are set, it prefers `GITHUB_TOKEN`.
- Prefer tokens with an expiration date; rotate if leaked.

Example:

- `GITHUB_TOKEN=... ./orchestrator release-iso --version v1.2.3`

Version format:

- `--version` must be SemVer.
- Accepted examples: `1.2.3`, `v1.2.3`, `1.2.3-rc.1`, `v1.2.3-rc.1`
- The `v` prefix is optional; orchestrator will normalize tags to `v{semver}`.

Resume example (if some tags/releases already exist):

- `GITHUB_TOKEN=... ./orchestrator release-iso --version v1.2.3 --resume`

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
