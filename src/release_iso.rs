use crate::git::Repo;
use crate::github::GitHub;
use crate::reporter::DynReporter;
use anyhow::{Context, Result, bail};
use semver::Version;
use std::path::{Path, PathBuf};
use std::time::Duration;

#[derive(Clone, Debug)]
pub struct ReleaseIsoArgs {
    pub version: String,
    pub repos_root: Option<PathBuf>,
    pub owner: String,
    pub dry_run: bool,
    pub resume: bool,
    pub poll_interval: Duration,
    pub timeout: Duration,
}

fn parse_and_normalize_version(input: &str) -> Result<(String, String)> {
    // Accept inputs like:
    // - 1.2.3
    // - v1.2.3
    // - 1.2.3-rc.1
    // - v1.2.3-rc.1
    // - 1.2.3+build.5
    // - v1.2.3+build.5
    //
    // Normalize everything to a tag `v{semver}` and a version string `{semver}`.
    let without_v = input.strip_prefix('v').unwrap_or(input);

    // Avoid confusing inputs like "vv1.2.3"; this almost always indicates a typo.
    if input.starts_with('v') && without_v.starts_with('v') {
        bail!("invalid version '{input}': remove extra leading 'v'. Example: v1.2.3");
    }

    let parsed = Version::parse(without_v).with_context(|| {
        format!(
            "invalid version '{input}'. Expected SemVer like '1.2.3', 'v1.2.3', '1.2.3-rc.1', or 'v1.2.3-rc.1'"
        )
    })?;

    let version = parsed.to_string();
    let tag = format!("v{version}");
    Ok((tag, version))
}

fn default_repos_root() -> Result<PathBuf> {
    let cwd = std::env::current_dir().context("failed to read current directory")?;

    if looks_like_repos_root(&cwd) {
        return Ok(cwd);
    }

    let parent = cwd
        .parent()
        .map(Path::to_path_buf)
        .context("can't infer repos root; run from repo root or pass --repos-root")?;

    if looks_like_repos_root(&parent) {
        return Ok(parent);
    }

    bail!(
        "can't infer repos root from {}. Pass --repos-root pointing to the directory containing truthdb/, installer/, installer-kernel/, installer-iso/",
        cwd.display()
    )
}

fn looks_like_repos_root(dir: &Path) -> bool {
    [
        "truthdb",
        "installer",
        "installer-kernel",
        "installer-iso",
        "truthdb-net",
        "truthdb-proto",
    ]
    .iter()
    .all(|name| dir.join(name).is_dir())
}

fn expected_assets(repo: &str, version_without_v: &str) -> Vec<String> {
    match repo {
        "installer-kernel" => vec!["BOOTX64.EFI".to_string()],
        "installer" => vec![
            format!(
                "truthdb-installer-v{}-x86_64-linux-musl.tar.gz",
                version_without_v
            ),
            format!(
                "truthdb-installer-v{}-x86_64-linux-musl.sha256",
                version_without_v
            ),
        ],
        "truthdb" => vec![
            format!("truthdb-v{}-x86_64-linux-gnu.tar.gz", version_without_v),
            format!("truthdb-v{}-x86_64-linux-gnu.sha256", version_without_v),
        ],
        "truthdb-cli" => vec![
            format!("truthdb-cli-v{}-x86_64-linux-gnu.tar.gz", version_without_v),
            format!("truthdb-cli-v{}-x86_64-linux-gnu.sha256", version_without_v),
        ],
        "truthdb-net" => vec![
            format!("truthdb-net-v{}-x86_64-linux-gnu.tar.gz", version_without_v),
            format!("truthdb-net-v{}-x86_64-linux-gnu.sha256", version_without_v),
        ],
        "truthdb-proto" => vec![
            format!(
                "truthdb-proto-v{}-x86_64-linux-gnu.tar.gz",
                version_without_v
            ),
            format!(
                "truthdb-proto-v{}-x86_64-linux-gnu.sha256",
                version_without_v
            ),
        ],
        "installer-iso" => vec![
            format!("truthdb-installer-v{}.iso", version_without_v),
            format!("truthdb-installer-v{}.iso.sha256", version_without_v),
        ],
        _ => Vec::new(),
    }
}

pub fn run(args: ReleaseIsoArgs, reporter: DynReporter) -> Result<()> {
    let (tag, version_without_v) = parse_and_normalize_version(&args.version)?;

    reporter.step(
        "Initialize".to_string(),
        format!(
            "version={} (tag={})\nmode={}{}",
            version_without_v,
            tag,
            if args.dry_run { "dry-run" } else { "live" },
            if args.resume { ", resume" } else { "" }
        ),
    );

    let repos_root = match args.repos_root {
        Some(p) => p,
        None => default_repos_root()?,
    };

    reporter.update(format!("repos_root={}", repos_root.display()));

    let repos_in_order = [
        "installer-kernel",
        "installer",
        "truthdb",
        "truthdb-cli",
        "truthdb-net",
        "truthdb-proto",
        "installer-iso",
    ];

    let repos: Vec<Repo> = repos_in_order
        .iter()
        .map(|name| Repo::new(&args.owner, *name, repos_root.join(name)))
        .collect();

    // Preflight: do all safety checks up-front before we mutate anything.
    // In --resume mode, we only require strict "A" checks on repos that are not
    // already tagged on origin.
    let mut remote_tagged: std::collections::BTreeMap<String, bool> =
        std::collections::BTreeMap::new();

    for repo in &repos {
        reporter.step(
            format!("Preflight [{}]", repo.name),
            format!("Checking repo at {}", repo.dir.display()),
        );

        if !repo.dir.is_dir() {
            bail!("repo directory not found: {}", repo.dir.display());
        }

        reporter.update("Verifying origin remote…".to_string());
        repo.ensure_origin_matches_expected()?;

        // Always fetch so remote tag and branch comparisons are reliable.
        reporter.update("Fetching origin tags…".to_string());
        repo.fetch_origin()?;

        reporter.update(format!("Checking remote tag {}…", tag));
        let is_remote_tagged = repo.remote_tag_commit(&tag)?.is_some();
        remote_tagged.insert(repo.name.clone(), is_remote_tagged);

        if is_remote_tagged {
            if args.resume {
                // Tag already exists on origin; don't block resume due to local state.
                continue;
            }
            bail!(
                "{} already has remote tag {tag} on origin. Re-run with --resume to continue.",
                repo.dir.display()
            );
        }

        // Not yet tagged on origin: enforce strict "A" safety checks.
        reporter.update("Ensuring worktree clean…".to_string());
        repo.ensure_worktree_clean()?;

        reporter.update("Ensuring branch is synced with origin…".to_string());
        let _branch = repo.ensure_on_branch_and_synced_to_origin()?;

        // In resume mode, allow a pre-existing local tag only if it points at HEAD.
        if args.resume {
            if let Some(local_tag_commit) = repo.local_tag_commit(&tag)? {
                let head_commit = repo.head_commit()?;

                if local_tag_commit != head_commit {
                    bail!(
                        "{} already has local tag {tag}, but it does not point at HEAD (tag={}, head={}). Refusing to push; delete/fix the local tag or choose a new version.",
                        repo.dir.display(),
                        local_tag_commit,
                        head_commit
                    );
                }
            }
        } else {
            reporter.update("Ensuring local/remote tag absent…".to_string());
            repo.ensure_tag_absent_local_and_remote(&tag)?;
        }
    }

    let token = std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("GH_TOKEN"))
        .unwrap_or_default();

    if !args.dry_run && token.is_empty() {
        bail!(
            "missing GITHUB_TOKEN (or GH_TOKEN). This is required to poll release assets after tagging."
        );
    }

    let gh = if args.dry_run || token.is_empty() {
        None
    } else {
        Some(GitHub::new(args.owner.clone(), token)?)
    };

    for repo in &repos {
        let already_remote_tagged = *remote_tagged.get(&repo.name).unwrap_or(&false);
        reporter.step(format!("Tagging [{}]", repo.name), format!("tag={}", tag));

        if args.dry_run {
            if already_remote_tagged {
                reporter.update(format!(
                    "[{}] (dry-run) tag already on origin; would skip tagging",
                    repo.name
                ));
            } else {
                reporter.update(format!(
                    "[{}] (dry-run) would create annotated tag and push",
                    repo.name
                ));
            }
        } else if already_remote_tagged {
            reporter.update(format!(
                "[{}] tag already exists on origin; skipping create/push",
                repo.name
            ));
        } else {
            // Create tag if it doesn't already exist locally; in --resume mode it may.
            if repo.local_tag_commit(&tag)?.is_none() {
                reporter.update("Creating annotated tag…".to_string());
                repo.create_annotated_tag(&tag)?;
            }

            reporter.update("Pushing tag to origin…".to_string());
            repo.push_tag(&tag)?;
        }

        let expected = expected_assets(&repo.name, &version_without_v);
        if expected.is_empty() {
            continue;
        }

        if args.dry_run {
            reporter.update(format!(
                "[{}] (dry-run) would wait for assets: {:?}",
                repo.name, expected
            ));
        } else if let Some(ref gh) = gh {
            reporter.step(
                format!("Waiting for assets [{}]", repo.name),
                format!("expected={:?}", expected),
            );
            gh.wait_for_release_assets(
                &repo.name,
                &tag,
                &expected,
                args.poll_interval,
                args.timeout,
                reporter.as_ref(),
            )
            .with_context(|| format!("waiting for {} assets", repo.name))?;
        }
    }

    reporter.step(
        "Complete".to_string(),
        format!("All done. installer-iso release should now produce the ISO for {tag}."),
    );
    reporter.ok("OK".to_string());
    Ok(())
}
