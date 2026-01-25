mod git;
mod github;
mod release_iso;
mod reporter;
mod tui;

use anyhow::Result;
use clap::{Parser, Subcommand};
use reporter::{DynReporter, PlainReporter};
use std::path::PathBuf;
use std::time::Duration;
use std::{io::IsTerminal, sync::Arc};

#[derive(Parser, Debug)]
#[command(name = "orchestrator")]
#[command(about = "Admin tools for the TruthDB organization")]
struct Cli {
    /// Disable the ratatui UI (use plain stderr output).
    #[arg(long, default_value_t = false)]
    no_tui: bool,

    /// Exit automatically when the command completes successfully (TUI mode only).
    #[arg(long, default_value_t = false)]
    auto_exit: bool,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Tag and release all dependencies needed to produce an installer ISO.
    ///
    /// This tags local repos and pushes tags to origin. It then polls GitHub Releases
    /// until required assets are present and stable before proceeding.
    ReleaseIso {
        /// Version/tag to create (SemVer).
        ///
        /// Examples: v1.2.3, 1.2.3, v1.2.3-rc.1, 1.2.3-rc.1
        #[arg(long)]
        version: String,

        /// Directory containing the sibling repos (truthdb/, installer/, installer-kernel/, installer-iso/).
        #[arg(long)]
        repos_root: Option<PathBuf>,

        /// GitHub org/owner.
        #[arg(long, default_value = "Truthdb")]
        owner: String,

        /// Don't create or push tags; just print what would happen.
        #[arg(long, default_value_t = false)]
        dry_run: bool,

        /// Resume a partially completed release (skip tag creation/push for repos
        /// that already have the tag on origin, but still poll assets and continue).
        #[arg(long, default_value_t = false)]
        resume: bool,

        /// Poll interval in seconds.
        #[arg(long, default_value_t = 10)]
        poll_interval_secs: u64,

        /// Timeout in seconds per repo while waiting for release assets.
        #[arg(long, default_value_t = 45 * 60)]
        timeout_secs: u64,
    },
}

fn main() -> Result<()> {
    let cli = Cli::parse();

    let use_tui = !cli.no_tui && std::io::stdout().is_terminal() && std::io::stderr().is_terminal();

    if use_tui {
        let (tx, rx) = crossbeam_channel::unbounded();
        let reporter: DynReporter = Arc::new(reporter::ChannelReporter::new(tx.clone()));
        reporter.step(
            "Initializing".to_string(),
            "Starting orchestrator…".to_string(),
        );
        reporter.ok("OK".to_string());

        // Move the command into a worker thread so the UI can stay responsive.
        let command = cli.command;
        let worker = std::thread::spawn({
            let reporter = reporter.clone();
            let tx = tx.clone();
            move || {
                let result = run_command(command, reporter.clone());
                if let Err(ref e) = result {
                    reporter.step(
                        "Failed".to_string(),
                        "An error occurred. See Status for details. Press q to quit.".to_string(),
                    );
                    reporter.error(format!("{e:#}"));
                }
                let _ = tx.send(crate::tui::UiEvent::Finished { ok: result.is_ok() });
                result
            }
        });

        // Run the UI loop on the main thread.
        let ui_res = tui::run(rx, cli.auto_exit);

        // Ensure worker has completed; if it errored, print a normal error after UI teardown.
        let worker_res = match worker.join() {
            Ok(r) => r,
            Err(_) => Err(anyhow::anyhow!("worker thread panicked")),
        };

        // Prefer surfacing any UI init errors too.
        ui_res?;

        if let Err(e) = worker_res {
            // Preserve the traditional non-TUI error output for logs / copy-paste.
            eprintln!("{e:?}");
            std::process::exit(1);
        }

        return Ok(());
    }

    let reporter: DynReporter = Arc::new(PlainReporter::new());
    run_command(cli.command, reporter)
}

fn run_command(command: Commands, reporter: DynReporter) -> Result<()> {
    match command {
        Commands::ReleaseIso {
            version,
            repos_root,
            owner,
            dry_run,
            resume,
            poll_interval_secs,
            timeout_secs,
        } => release_iso::run(
            release_iso::ReleaseIsoArgs {
                version,
                repos_root,
                owner,
                dry_run,
                resume,
                poll_interval: Duration::from_secs(poll_interval_secs),
                timeout: Duration::from_secs(timeout_secs),
            },
            reporter,
        ),
    }
}
