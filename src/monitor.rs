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
    github::GitHub,
    reporter::DynReporter,
    tui::{ActionState, RepoStatusRow, UiEvent},
};

#[derive(Clone, Debug)]
pub struct MonitorArgs {
    pub owner: String,
    pub poll_interval: Duration,
}

const REPOS: [&str; 12] = [
    ".github",
    "docs",
    "installer",
    "installer-iso",
    "installer-kernel",
    "installer-kernel-builder-image",
    "orchestrator",
    "truthdb",
    "truthdb-cli",
    "truthdb-net",
    "truthdb-proto",
    "website",
];

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

    let token = std::env::var("GITHUB_TOKEN")
        .or_else(|_| std::env::var("GH_TOKEN"))
        .unwrap_or_default();

    let has_token = !token.is_empty();

    if !has_token {
        reporter.error(
            "Missing GITHUB_TOKEN (or GH_TOKEN). Repo status will likely be rate-limited/unauthenticated."
                .to_string(),
        );
    } else {
        reporter.ok("OK".to_string());
    }

    let gh = GitHub::new(args.owner, token)?;

    // Initial paint: list all repos immediately with a loading indicator, then fill them in.
    let mut rows = placeholder_rows();
    let _ = tx.send(UiEvent::SetRepos { rows: rows.clone() });
    refresh_rows_incremental(&gh, &mut rows, &tx, true)?;

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

        match refresh_rows_incremental(&gh, &mut rows, &tx, false) {
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

        let default_branch = gh
            .get_default_branch(repo)
            .unwrap_or_else(|_| "main".to_string());

        let action = match gh.get_latest_workflow_run(repo) {
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
            Ok(None) | Err(_) => ActionState::Unknown,
        };

        let release_tag = match gh.get_latest_release_tag(repo) {
            Ok(Some(tag)) => Some(tag),
            Ok(None) | Err(_) => None,
        };

        let ahead_by = match release_tag.as_deref() {
            Some(tag) => gh.compare_ahead_by(repo, tag, &default_branch).ok(),
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
    }

    Ok(())
}
