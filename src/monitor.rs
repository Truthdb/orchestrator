use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::Result;
use crossbeam_channel::Sender;

use crate::{
    github::{
        FALLBACK_GITHUB_TOKEN_ENV, GitHub, LEGACY_GITHUB_TOKEN_ENV, PRIMARY_GITHUB_TOKEN_ENV,
        github_token,
    },
    reporter::DynReporter,
    tui::{ActionState, RepoStatusRow, UiEvent},
};

#[derive(Clone, Debug)]
pub struct MonitorArgs {
    pub owner: String,
    pub poll_interval: Duration,
}

const REPOS: [&str; 9] = [
    ".github",
    "docs",
    "installer",
    "installer-iso",
    "installer-kernel",
    "installer-kernel-builder-image",
    "orchestrator",
    "truthdb",
    "website",
];

const CI_WORKFLOW_FILE: &str = "ci.yml";

pub fn run(
    args: MonitorArgs,
    tx: Sender<UiEvent>,
    reporter: DynReporter,
    shutdown: Arc<AtomicBool>,
) -> Result<()> {
    reporter.step(
        "Monitor".to_string(),
        format!(
            "owner={}\nrepos={}\nrefresh={}s",
            args.owner,
            REPOS.len(),
            args.poll_interval.as_secs()
        ),
    );

    let token = github_token();

    let has_token = !token.is_empty();

    if !has_token {
        reporter.error(format!(
            "Missing {}, {}, or {}. Repo status will likely be rate-limited/unauthenticated.",
            PRIMARY_GITHUB_TOKEN_ENV, FALLBACK_GITHUB_TOKEN_ENV, LEGACY_GITHUB_TOKEN_ENV
        ));
    } else {
        reporter.ok("OK".to_string());
    }

    let gh = GitHub::new(args.owner, token)?;

    // Initial paint: list all repos immediately with a loading indicator, then fill them in.
    let mut rows = placeholder_rows();
    let _ = tx.send(UiEvent::SetRepos { rows: rows.clone() });
    refresh_rows_incremental(&gh, &mut rows, &tx, reporter.as_ref(), true)?;

    while !shutdown.load(Ordering::SeqCst) {
        let mut slept = Duration::ZERO;
        while slept < args.poll_interval && !shutdown.load(Ordering::SeqCst) {
            let step = Duration::from_millis(200);
            std::thread::sleep(step);
            slept += step;
        }

        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        match refresh_rows_incremental(&gh, &mut rows, &tx, reporter.as_ref(), false) {
            Ok(()) => {
                if has_token {
                    reporter.ok("OK".to_string());
                }
            }
            Err(e) => {
                reporter.error(format!("Monitor refresh failed: {e:#}"));
            }
        }
    }

    Ok(())
}

fn placeholder_rows() -> Vec<RepoStatusRow> {
    REPOS
        .iter()
        .map(|repo| RepoStatusRow {
            name: (*repo).to_string(),
            action: ActionState::Unknown,
            latest_release: None,
            ahead_by: None,
            loading: true,
        })
        .collect()
}

fn refresh_rows_incremental(
    gh: &GitHub,
    rows: &mut [RepoStatusRow],
    tx: &Sender<UiEvent>,
    reporter: &dyn crate::reporter::Reporter,
    show_loading: bool,
) -> Result<()> {
    if show_loading {
        for row in rows.iter_mut() {
            row.loading = true;
        }
        let _ = tx.send(UiEvent::SetRepos {
            rows: rows.to_vec(),
        });
    }

    for (i, repo) in REPOS.iter().enumerate() {
        if let Some(row) = rows.get_mut(i) {
            row.loading = show_loading;
        }

        let mut repo_errors = Vec::new();

        let default_branch = match gh.get_default_branch(repo) {
            Ok(branch) => branch,
            Err(err) => {
                repo_errors.push(format!("default branch: {err:#}"));
                "main".to_string()
            }
        };

        let action = match gh.get_latest_workflow_run(repo, CI_WORKFLOW_FILE, &default_branch) {
            Ok(Some(run)) => {
                if run.status == "completed" {
                    match run.conclusion.as_deref() {
                        Some("success") => ActionState::Success,
                        Some("failure") | Some("cancelled") | Some("timed_out") => {
                            ActionState::Failure
                        }
                        Some(_) | None => ActionState::Unknown,
                    }
                } else {
                    ActionState::Running
                }
            }
            Ok(None) => ActionState::Unknown,
            Err(err) => {
                repo_errors.push(format!("CI workflow runs: {err:#}"));
                ActionState::Unknown
            }
        };

        let release_tag = match gh.get_latest_release_tag(repo) {
            Ok(Some(tag)) => Some(tag),
            Ok(None) => None,
            Err(err) => {
                repo_errors.push(format!("latest release: {err:#}"));
                None
            }
        };

        let ahead_by = match release_tag.as_deref() {
            Some(tag) => match gh.compare_ahead_by(repo, tag, &default_branch) {
                Ok(ahead_by) => Some(ahead_by),
                Err(err) => {
                    repo_errors.push(format!("ahead-by compare: {err:#}"));
                    None
                }
            },
            None => None,
        };

        if let Some(row) = rows.get_mut(i) {
            row.action = action;
            row.latest_release = release_tag;
            row.ahead_by = ahead_by;
            row.loading = false;
        }

        // Update the UI as each repo completes (keeps existing values visible between refreshes).
        let _ = tx.send(UiEvent::SetRepos {
            rows: rows.to_vec(),
        });

        if !repo_errors.is_empty() {
            reporter.error(format!("[{}] {}", repo, repo_errors.join(" | ")));
        }
    }

    Ok(())
}
