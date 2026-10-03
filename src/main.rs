use std::{env, path::Path};

use clap::Parser;

mod dependabot;
mod ecosystems;

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {
    #[arg(
        long,
        help = "Combine all npm workspace dependencies into one Dependabot update entry"
    )]
    combine_workspaces: bool,
}

fn main() {
    let cli = Cli::parse();

    let current_path = env::current_dir().unwrap();
    let Some(project) = ecosystems::detect(&current_path).unwrap() else {
        return;
    };

    dependabot::write_config_file(
        Path::new(".github/dependabot.yml"),
        project.package_ecosystem,
        &project.groups_by_workspace,
        cli.combine_workspaces,
    )
    .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_combine_workspaces_option() {
        let cli = Cli::try_parse_from(["dependaconf", "--combine-workspaces"]).unwrap();

        assert!(cli.combine_workspaces);
    }
}
