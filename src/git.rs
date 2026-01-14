use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

fn run_git(repo_dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .current_dir(repo_dir)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {args:?} in {}", repo_dir.display()))?;

    if !output.status.success() {
        let stdout = String::from_utf8_lossy(&output.stdout);
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "git {args:?} failed in {} (exit={}):\nstdout:\n{}\nstderr:\n{}",
            repo_dir.display(),
            output.status,
            stdout.trim_end(),
            stderr.trim_end()
        );
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn run_git_status(repo_dir: &Path, args: &[&str]) -> Result<(i32, String, String)> {
    let output = Command::new("git")
        .current_dir(repo_dir)
        .args(args)
        .output()
        .with_context(|| format!("failed to run git {args:?} in {}", repo_dir.display()))?;

    let code = output.status.code().unwrap_or(-1);
    let stdout = String::from_utf8_lossy(&output.stdout).trim().to_string();
    let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Ok((code, stdout, stderr))
}

#[derive(Clone, Debug)]
pub struct Repo {
    pub name: String,
    pub dir: PathBuf,
    pub owner: String,
}

impl Repo {
    pub fn new(owner: impl Into<String>, name: impl Into<String>, dir: impl Into<PathBuf>) -> Self {
        Self {
            name: name.into(),
            dir: dir.into(),
            owner: owner.into(),
        }
    }

    pub fn ensure_worktree_clean(&self) -> Result<()> {
        let status = run_git(&self.dir, &["status", "--porcelain"])?;
        if !status.is_empty() {
            bail!(
                "{} has uncommitted changes:\n{}\n\nCommit/stash them before releasing.",
                self.dir.display(),
                status
            );
        }
        Ok(())
    }

    pub fn fetch_origin(&self) -> Result<()> {
        run_git(&self.dir, &["fetch", "--tags", "origin"]).context("git fetch origin failed")?;
        Ok(())
    }

    pub fn head_commit(&self) -> Result<String> {
        run_git(&self.dir, &["rev-parse", "HEAD"]).context("failed to resolve HEAD")
    }

    pub fn ensure_on_branch_and_synced_to_origin(&self) -> Result<String> {
        // Fetch first so origin/<branch> is up to date.
        self.fetch_origin()?;

        // Fail if detached.
        let (code, branch, _) = run_git_status(&self.dir, &["symbolic-ref", "--quiet", "--short", "HEAD"])?;
        if code != 0 || branch.is_empty() {
            bail!(
                "{} is in detached HEAD state; check out a branch first.",
                self.dir.display()
            );
        }

        let local_head = run_git(&self.dir, &["rev-parse", "HEAD"])?;
        let remote_ref = format!("origin/{branch}");
        let remote_head = run_git(&self.dir, &["rev-parse", &remote_ref])
            .with_context(|| format!("failed to resolve {remote_ref} in {}", self.dir.display()))?;

        if local_head != remote_head {
            let counts = run_git(&self.dir, &["rev-list", "--left-right", "--count", &format!("HEAD...{remote_ref}")])
                .unwrap_or_default();
            bail!(
                "{} is not synced with {}.\nlocal HEAD:  {}\nremote HEAD: {}\n(diverged counts: {})\n\nPlease `git pull` / fast-forward your branch before tagging.",
                self.dir.display(),
                remote_ref,
                local_head,
                remote_head,
                counts
            );
        }

        Ok(branch)
    }

    pub fn ensure_origin_matches_expected(&self) -> Result<()> {
        let url = run_git(&self.dir, &["remote", "get-url", "origin"])?;
        // Accept both SSH and HTTPS; just sanity-check that owner/repo appear.
        let needle = format!("{}/{}", self.owner, self.name);
        if !url.to_lowercase().contains(&needle.to_lowercase()) {
            bail!(
                "{} origin remote doesn't look like {} (got: {}). Refusing to push tags.",
                self.dir.display(),
                needle,
                url
            );
        }
        Ok(())
    }

    pub fn ensure_tag_absent_local_and_remote(&self, tag: &str) -> Result<()> {
        let local = run_git(&self.dir, &["tag", "-l", tag])?;
        if !local.is_empty() {
            bail!("{} already has local tag {tag}", self.dir.display());
        }

        let refname = format!("refs/tags/{tag}");
        let (code, _stdout, stderr) = run_git_status(
            &self.dir,
            &["ls-remote", "--exit-code", "--tags", "origin", &refname],
        )?;
        if code == 0 {
            bail!("{} already has remote tag {tag} on origin", self.dir.display());
        }

        // `git ls-remote --exit-code` uses exit code 2 to indicate "not found".
        // Any other non-zero likely indicates a real error (network/auth/etc).
        if code != 2 {
            bail!(
                "{} failed to query remote tags (exit={code}): {stderr}",
                self.dir.display()
            );
        }

        Ok(())
    }

    pub fn local_tag_commit(&self, tag: &str) -> Result<Option<String>> {
        // `rev-parse --verify` exits non-zero if it doesn't exist.
        let (code, _stdout, _stderr) = run_git_status(&self.dir, &["rev-parse", "--verify", tag])?;
        if code != 0 {
            return Ok(None);
        }

        // Resolve tag to the commit it ultimately points at.
        let commit = run_git(&self.dir, &["rev-list", "-n", "1", tag])?;
        if commit.is_empty() {
            return Ok(None);
        }
        Ok(Some(commit))
    }

    pub fn remote_tag_commit(&self, tag: &str) -> Result<Option<String>> {
        let refname = format!("refs/tags/{tag}^{{}}");
        let (code, stdout, stderr) = run_git_status(&self.dir, &["ls-remote", "--tags", "origin", &refname])?;

        // Any non-zero here usually indicates a real error (network/auth).
        if code != 0 {
            bail!(
                "{} failed to query remote tag {tag} (exit={code}): {stderr}",
                self.dir.display()
            );
        }

        // Output format: "<sha>\t<ref>".
        let sha = stdout.split_whitespace().next().unwrap_or("").to_string();
        if sha.is_empty() {
            return Ok(None);
        }
        Ok(Some(sha))
    }

    pub fn create_annotated_tag(&self, tag: &str) -> Result<()> {
        let msg = format!("Release {tag}");
        let _ = run_git(&self.dir, &["tag", "-a", tag, "-m", &msg])?;
        Ok(())
    }

    pub fn push_tag(&self, tag: &str) -> Result<()> {
        let _ = run_git(&self.dir, &["push", "origin", tag])?;
        Ok(())
    }
}
