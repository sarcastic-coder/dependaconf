use std::{env, error::Error as StdError, fmt, io, path::Path};

use clap::Parser;

mod dependabot;
mod ecosystems;

#[derive(Debug)]
enum MainError {
    CurrentDirectory(io::Error),
    Ecosystems(ecosystems::Error),
    Dependabot(dependabot::Error),
}

impl fmt::Display for MainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CurrentDirectory(error) => {
                write!(f, "failed to determine the current directory: {error}")
            }
            Self::Ecosystems(error) => error.fmt(f),
            Self::Dependabot(error) => error.fmt(f),
        }
    }
}

impl StdError for MainError {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::CurrentDirectory(error) => Some(error),
            Self::Ecosystems(error) => Some(error),
            Self::Dependabot(error) => Some(error),
        }
    }
}

impl From<ecosystems::Error> for MainError {
    fn from(error: ecosystems::Error) -> Self {
        Self::Ecosystems(error)
    }
}

impl From<dependabot::Error> for MainError {
    fn from(error: dependabot::Error) -> Self {
        Self::Dependabot(error)
    }
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(
        long,
        help = "Combine all npm workspace dependencies into one Dependabot update entry"
    )]
    combine_workspaces: bool,

    #[arg(long, help = "Print dependency groups and peer links for debugging")]
    debug: bool,
}

fn main() -> Result<(), MainError> {
    let cli = Cli::parse();

    let current_path = env::current_dir().map_err(MainError::CurrentDirectory)?;
    let project = match ecosystems::detect(&current_path, cli.debug)? {
        ecosystems::Detection::Unsupported => return Ok(()),
        ecosystems::Detection::Detected(project) => project,
    };

    if cli.debug && cli.combine_workspaces {
        eprint!(
            "{}",
            dependabot::combined_groups_debug_report(&project.groups_by_workspace)?
        );
    } else if let Some(debug_report) = &project.debug_report {
        eprint!("{debug_report}");
    }

    dependabot::write_config_file(
        Path::new(".github/dependabot.yml"),
        project.package_ecosystem,
        &project.groups_by_workspace,
        cli.combine_workspaces,
    )?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combine_workspaces_option() {
        let cli = Cli::try_parse_from(["dependaconf", "--combine-workspaces"]).unwrap();

        assert!(cli.combine_workspaces);
    }

    #[test]
    fn parses_debug_option() {
        let cli = Cli::try_parse_from(["dependaconf", "--debug"]).unwrap();

        assert!(cli.debug);
    }
}
