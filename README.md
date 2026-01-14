# orchestrator
The command line tool to adminster the truthdb organisation

## Commands

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
- `GITHUB_TOKEN` (or `GH_TOKEN`) set for polling GitHub Releases.

Example:

- `GITHUB_TOKEN=... ./orchestrator release-iso --version v1.2.3`

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
