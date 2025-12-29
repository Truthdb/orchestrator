use anyhow::Result;
use clap::{Command, error::ErrorKind};

fn main() -> Result<()> {
    let cmd = Command::new("orchestrator")
        .disable_version_flag(true)
        .disable_help_subcommand(true);

    // If invoked without arguments, print usage/help.
    if std::env::args_os().len() == 1 {
        cmd.clone().print_help()?;
        println!();
        return Ok(());
    }

    // The only supported flags are `-h/--help`. Any other arg is an error.
    match cmd.clone().try_get_matches() {
        Ok(_) => Ok(()),
        Err(err) => {
            err.print()?;
            if matches!(
                err.kind(),
                ErrorKind::DisplayHelp | ErrorKind::DisplayVersion
            ) {
                Ok(())
            } else {
                std::process::exit(err.exit_code());
            }
        }
    }
}
