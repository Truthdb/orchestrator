mod git;
mod github;
mod release_iso;

use anyhow::Result;
use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::time::Duration;

#[derive(Parser, Debug)]
#[command(name = "orchestrator")]
#[command(about = "Admin tools for the TruthDB organization")]
struct Cli {
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

    match cli.command {
        Commands::ReleaseIso {
            version,
            repos_root,
            owner,
            dry_run,
            resume,
            poll_interval_secs,
            timeout_secs,
        } => {
            release_iso::run(release_iso::ReleaseIsoArgs {
                version,
                repos_root,
                owner,
                dry_run,
                resume,
                poll_interval: Duration::from_secs(poll_interval_secs),
                timeout: Duration::from_secs(timeout_secs),
            })?;
        }
    }

    Ok(())
}
