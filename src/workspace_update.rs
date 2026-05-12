use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result, anyhow, bail};
use include_dir::{Dir, DirEntry, include_dir};
use serde::Deserialize;

use crate::git::clone_repo;
use crate::github::GitHub;
use crate::reporter::DynReporter;

static WORKSPACE_DIR: Dir<'_> = include_dir!("$CARGO_MANIFEST_DIR/workspace");

const MANIFEST_PATH: &str = "repos.toml";
const BIN_DIR_NAME: &str = ".bin";
const WRAPPER_NAME: &str = "orchestrator";
const INSTALLED_BINARY_NAME: &str = ".orchestrator-bin";

#[derive(Debug, Clone)]
pub struct WorkspaceUpdateArgs {
    pub workspace_root: Option<PathBuf>,
    pub owner: String,
}

#[derive(Debug, Deserialize)]
struct WorkspaceManifest {
    repos: Vec<String>,
}

pub fn run(args: WorkspaceUpdateArgs, reporter: DynReporter) -> Result<()> {
    let workspace_root = resolve_workspace_root(args.workspace_root)?;
    fs::create_dir_all(&workspace_root).with_context(|| {
        format!(
            "failed to create workspace root at {}",
            workspace_root.display()
        )
    })?;

    let manifest = load_manifest()?;
    // Fail fast with a clear message if the token isn't set. Without this,
    // a missing/typoed token only surfaces if a repo actually needs cloning,
    // and even then via a generic 401 round-trip — easy to miss.
    let github = GitHub::new(args.owner.clone(), crate::github::require_github_token()?)?;

    reporter.step(
        "Cloning repos".to_string(),
        format!("workspace_root={}", workspace_root.display()),
    );
    let cloned = clone_missing_repos(&workspace_root, &args.owner, &manifest, &github, &reporter)?;

    reporter.step("Syncing workspace files".to_string(), String::new());
    let synced = sync_workspace_files(&workspace_root, &reporter)?;

    reporter.step("Installing launcher".to_string(), String::new());
    let launcher_updated = install_launcher(&workspace_root, &reporter)?;

    reporter.ok(format!(
        "workspace ready (cloned={}, synced_files={}, launcher_updated={})",
        cloned, synced, launcher_updated
    ));
    Ok(())
}

fn load_manifest() -> Result<WorkspaceManifest> {
    let manifest_file = WORKSPACE_DIR
        .get_file(MANIFEST_PATH)
        .ok_or_else(|| anyhow!("embedded workspace manifest {} is missing", MANIFEST_PATH))?;
    let manifest_text = std::str::from_utf8(manifest_file.contents())
        .context("workspace manifest is not valid UTF-8")?;
    toml::from_str(manifest_text).context("failed to parse embedded workspace manifest")
}

fn resolve_workspace_root(explicit: Option<PathBuf>) -> Result<PathBuf> {
    if let Some(path) = explicit {
        return absolutize(path);
    }

    let cwd = env::current_dir().context("failed to get current directory")?;
    if let Some(root) = find_workspace_root_from_path(&cwd) {
        return Ok(root);
    }

    let exe = env::current_exe().context("failed to resolve current executable")?;
    if let Some(root) = find_workspace_root_from_path(&exe) {
        return Ok(root);
    }

    bail!(
        "could not infer workspace root from {} or {}. Re-run with --workspace-root.",
        cwd.display(),
        exe.display()
    );
}

fn absolutize(path: PathBuf) -> Result<PathBuf> {
    if path.is_absolute() {
        return Ok(path);
    }

    Ok(env::current_dir()
        .context("failed to get current directory")?
        .join(path))
}

fn find_workspace_root_from_path(path: &Path) -> Option<PathBuf> {
    for ancestor in path.ancestors() {
        if looks_like_workspace_root(ancestor) {
            return Some(ancestor.to_path_buf());
        }
    }

    for ancestor in path.ancestors() {
        if looks_like_installed_bin_dir(ancestor) {
            return ancestor.parent().map(Path::to_path_buf);
        }
        if looks_like_orchestrator_repo(ancestor) {
            return ancestor.parent().map(Path::to_path_buf);
        }
    }

    None
}

fn looks_like_workspace_root(dir: &Path) -> bool {
    dir.join("orchestrator").join("Cargo.toml").is_file()
}

fn looks_like_orchestrator_repo(dir: &Path) -> bool {
    dir.join("src").join("main.rs").is_file()
        && dir.join("Cargo.toml").is_file()
        && fs::read_to_string(dir.join("Cargo.toml"))
            .map(|contents| contents.contains("name = \"orchestrator\""))
            .unwrap_or(false)
}

fn looks_like_installed_bin_dir(dir: &Path) -> bool {
    dir.file_name().and_then(|name| name.to_str()) == Some(BIN_DIR_NAME)
        && dir.join(INSTALLED_BINARY_NAME).is_file()
}

fn clone_missing_repos(
    workspace_root: &Path,
    owner: &str,
    manifest: &WorkspaceManifest,
    github: &GitHub,
    reporter: &DynReporter,
) -> Result<usize> {
    let mut cloned = 0usize;

    for repo in &manifest.repos {
        let repo_dir = workspace_root.join(repo);
        if repo_dir.exists() {
            if !repo_dir.is_dir() {
                bail!(
                    "expected {} to be a directory, but it already exists and is not a directory",
                    repo_dir.display()
                );
            }
            reporter.update(format!("repo {} already present", repo));
            continue;
        }

        // Emit a fresh step per repo that actually needs cloning so the TUI
        // timer resets and the title reflects which repo is in flight. We
        // intentionally skip the "already present" case so the timer doesn't
        // flicker for trivial work.
        reporter.step(
            format!("Cloning {repo}"),
            format!("validating GitHub repo {owner}/{repo}"),
        );
        let _ = github
            .get_default_branch(repo)
            .with_context(|| format!("failed to validate GitHub repo {owner}/{repo}"))?;

        let clone_url = format!("git@github.com:{owner}/{repo}.git");
        reporter.update(format!("cloning {} into {}", clone_url, repo_dir.display()));
        clone_repo(workspace_root, &clone_url, repo)?;
        cloned += 1;
    }

    Ok(cloned)
}

fn sync_workspace_files(workspace_root: &Path, reporter: &DynReporter) -> Result<usize> {
    let mut updated = 0usize;
    let source_root = workspace_root.join("orchestrator").join("workspace");

    // Prefer the live on-disk source over the snapshot embedded into this
    // binary at compile time. Without this, edits to `orchestrator/workspace/`
    // are invisible until the orchestrator is rebuilt AND the rebuilt binary
    // is the one being run via `.bin/.orchestrator-bin`. The embedded copy
    // is kept as a fallback for first-time bootstrap (when `orchestrator/`
    // hasn't been cloned yet).
    if source_root.is_dir() {
        reporter.update(format!("syncing from {}", source_root.display()));
        sync_disk_dir(&source_root, &source_root, workspace_root, &mut updated)?;
    } else {
        reporter.update("syncing from embedded snapshot (no on-disk workspace/ found)".to_string());
        sync_embedded_dir(&WORKSPACE_DIR, workspace_root, &source_root, &mut updated)?;
    }

    if updated == 0 {
        reporter.update("workspace files already current".to_string());
    } else {
        reporter.update(format!("updated {} workspace file(s)", updated));
    }

    Ok(updated)
}

fn sync_embedded_dir(
    dir: &Dir<'_>,
    workspace_root: &Path,
    source_root: &Path,
    updated: &mut usize,
) -> Result<()> {
    for entry in dir.entries() {
        match entry {
            DirEntry::Dir(child) => sync_embedded_dir(child, workspace_root, source_root, updated)?,
            DirEntry::File(file) => {
                if file.path() == Path::new(MANIFEST_PATH) {
                    continue;
                }

                let dest = workspace_root.join(file.path());
                let source = source_root.join(file.path());
                let wrote = write_if_changed(&dest, file.contents())?;
                let perms_changed = sync_file_permissions(&source, &dest)?;
                if wrote || perms_changed {
                    *updated += 1;
                }
            }
        }
    }

    Ok(())
}

/// Recursively walk `dir` and mirror it into `workspace_root`, using
/// `source_root` to compute the relative path under the destination. Skips
/// `repos.toml` (same as the embedded path) and anything under `.git/`.
fn sync_disk_dir(
    dir: &Path,
    source_root: &Path,
    workspace_root: &Path,
    updated: &mut usize,
) -> Result<()> {
    let entries =
        fs::read_dir(dir).with_context(|| format!("failed to read directory {}", dir.display()))?;

    for entry in entries {
        let entry = entry
            .with_context(|| format!("failed to read directory entry in {}", dir.display()))?;
        let path = entry.path();
        let file_name = entry.file_name();

        // Defensive: never sync version-control metadata.
        if file_name == ".git" {
            continue;
        }

        let file_type = entry
            .file_type()
            .with_context(|| format!("failed to stat {}", path.display()))?;

        if file_type.is_dir() {
            sync_disk_dir(&path, source_root, workspace_root, updated)?;
            continue;
        }

        if !file_type.is_file() {
            // Skip symlinks and anything exotic — the embedded path doesn't
            // support them either.
            continue;
        }

        let rel = path
            .strip_prefix(source_root)
            .with_context(|| format!("path {} escaped source root", path.display()))?;
        if rel == Path::new(MANIFEST_PATH) {
            continue;
        }

        let dest = workspace_root.join(rel);
        let contents =
            fs::read(&path).with_context(|| format!("failed to read source {}", path.display()))?;

        let wrote = write_if_changed(&dest, &contents)?;
        let perms_changed = sync_file_permissions(&path, &dest)?;
        if wrote || perms_changed {
            *updated += 1;
        }
    }

    Ok(())
}

/// Write `contents` to `dest`, creating parent directories as needed, only if
/// the destination is missing or its current bytes differ. Returns true when
/// a write actually happened.
fn write_if_changed(dest: &Path, contents: &[u8]) -> Result<bool> {
    if let Some(parent) = dest.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create directory {}", parent.display()))?;
    }

    let needs_write = match fs::read(dest) {
        Ok(existing) => existing != contents,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => true,
        Err(err) => {
            return Err(err).with_context(|| format!("failed to read {}", dest.display()));
        }
    };

    if needs_write {
        fs::write(dest, contents).with_context(|| format!("failed to write {}", dest.display()))?;
    }

    Ok(needs_write)
}

fn install_launcher(workspace_root: &Path, reporter: &DynReporter) -> Result<bool> {
    let bin_dir = workspace_root.join(BIN_DIR_NAME);
    fs::create_dir_all(&bin_dir)
        .with_context(|| format!("failed to create {}", bin_dir.display()))?;

    let current_exe = env::current_exe().context("failed to resolve current executable")?;
    let installed_binary = bin_dir.join(INSTALLED_BINARY_NAME);
    let wrapper = bin_dir.join(WRAPPER_NAME);

    let mut updated = false;

    if !same_file_path(&current_exe, &installed_binary) {
        let source = fs::read(&current_exe)
            .with_context(|| format!("failed to read {}", current_exe.display()))?;
        let existing = fs::read(&installed_binary).ok();
        if existing.as_deref() != Some(source.as_slice()) {
            fs::write(&installed_binary, &source)
                .with_context(|| format!("failed to write {}", installed_binary.display()))?;
            set_executable(&installed_binary)?;
            updated = true;
        }
    }

    let wrapper_contents = render_wrapper_script();
    let existing_wrapper = fs::read_to_string(&wrapper).ok();
    if existing_wrapper.as_deref() != Some(wrapper_contents.as_str()) {
        fs::write(&wrapper, wrapper_contents)
            .with_context(|| format!("failed to write {}", wrapper.display()))?;
        set_executable(&wrapper)?;
        updated = true;
    }

    if updated {
        reporter.update(format!(
            "installed workspace launcher at {}",
            wrapper.display()
        ));
    } else {
        reporter.update(format!(
            "workspace launcher already current at {}",
            wrapper.display()
        ));
    }

    Ok(updated)
}

fn render_wrapper_script() -> String {
    format!(
        "#!/usr/bin/env sh\nset -e\nSCRIPT_DIR=\"$(CDPATH= cd -- \"$(dirname \"$0\")\" && pwd)\"\nexec \"$SCRIPT_DIR/{}\" \"$@\"\n",
        INSTALLED_BINARY_NAME
    )
}

fn same_file_path(left: &Path, right: &Path) -> bool {
    match (fs::canonicalize(left), fs::canonicalize(right)) {
        (Ok(a), Ok(b)) => a == b,
        _ => left == right,
    }
}

fn set_executable(path: &Path) -> Result<()> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let mut perms = fs::metadata(path)
            .with_context(|| format!("failed to read metadata for {}", path.display()))?
            .permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms)
            .with_context(|| format!("failed to set permissions on {}", path.display()))?;
    }

    Ok(())
}

fn sync_file_permissions(source: &Path, dest: &Path) -> Result<bool> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        let source_mode = match fs::metadata(source) {
            Ok(metadata) => metadata.permissions().mode() & 0o777,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(false),
            Err(err) => {
                return Err(err)
                    .with_context(|| format!("failed to read metadata for {}", source.display()));
            }
        };

        let mut perms = fs::metadata(dest)
            .with_context(|| format!("failed to read metadata for {}", dest.display()))?
            .permissions();
        let dest_mode = perms.mode() & 0o777;

        if dest_mode != source_mode {
            perms.set_mode(source_mode);
            fs::set_permissions(dest, perms)
                .with_context(|| format!("failed to set permissions on {}", dest.display()))?;
            return Ok(true);
        }
    }

    Ok(false)
}
