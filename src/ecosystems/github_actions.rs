use std::{collections::HashMap, path::Path};

use crate::ecosystems::DependencyGroups;

pub(super) const DEPENDABOT_NAME: &str = "github-actions";

#[derive(Debug)]
pub(crate) enum Error {
    GlobPattern(String),
}

#[derive(serde::Deserialize)]
struct ActionStep {
    uses: Option<String>,
}

#[derive(serde::Deserialize)]
struct ActionJob {
    steps: Vec<ActionStep>,
    uses: Option<String>,
}

#[derive(serde::Deserialize)]
struct ActionWorkflow {
    jobs: Vec<ActionJob>,
}

pub(super) fn is_project(root: &Path) -> bool {
    root.join(".github/workflows").is_dir()
}

pub(super) fn dependency_analysis(_root: &Path) -> Result<DependencyGroups, super::Error> {
    let workflow_files = glob::glob(".github/workflows/**/*.{yml,yaml}").unwrap();

    for file in workflow_files {
        let path = file.as_ref().unwrap();
        if !path.is_file() {
            continue;
        }

        let mut dependencies: HashMap<String, usize> = HashMap::new();

        let content = std::fs::read_to_string(path).unwrap_or_default();
        let action: ActionWorkflow = serde_yaml::from_str(&content)
            .unwrap_or_else(|_| panic!("Failed to parse YAML file: {}", path.display()));

        action.jobs.iter().for_each(|job| {
            match &job.uses {
                Some(uses) => {
                    if dependencies.contains_key(uses) {
                        dependencies.get_mut(uses).map(|count| *count += 1);
                    } else {
                        dependencies.insert(uses.to_string(), 1);
                    }
                }
                None => {}
            }

            job.steps.iter().for_each(|step| match &step.uses {
                Some(uses) => {
                    if dependencies.contains_key(uses) {
                        dependencies.get_mut(uses).map(|count| *count += 1);
                    } else {
                        dependencies.insert(uses.to_string(), 1);
                    }
                }
                None => {}
            });
        });
    }

    Ok(DependencyGroups::default())
}

mod tests {
    use super::*;

    #[test]
    fn detects_github_actions_project() {
        let example_root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github-actions");
        assert!(is_project(&example_root));
    }

    #[test]
    fn detects_non_github_actions_project() {
        let example_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/empty");
        assert!(!is_project(&example_root));
    }

    #[test]
    fn analyzes_github_actions_dependencies() {
        let example_root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/github-actions");
        let groups = dependency_analysis(&example_root).unwrap();
        assert_eq!(groups.iter().count(), 0);
    }
}
