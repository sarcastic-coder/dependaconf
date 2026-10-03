use std::{env, path::Path};

use clap::{Parser, Subcommand};

mod dependabot;
mod ecosystems;

#[derive(Subcommand)]
enum Commands {
    Write {},
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {}

fn main() {
    let _ = Cli::parse();

    let current_path = env::current_dir().unwrap();
    let Some(project) = ecosystems::detect(&current_path).unwrap() else {
        return;
    };

    dependabot::write_config_file(
        Path::new(".github/dependabot.yml"),
        project.package_ecosystem,
        &project.groups_by_workspace,
    )
    .unwrap();
}
