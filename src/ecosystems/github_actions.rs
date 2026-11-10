use std::{
    collections::HashMap,
    error::Error as StdError,
    fmt, io,
    path::{Path, PathBuf},
};
use wax::{Glob, walk::Entry};

use crate::ecosystems::DependencyGroups;

pub(super) const DEPENDABOT_NAME: &str = "github-actions";

#[derive(Debug)]
pub(crate) enum Error {
    GlobPattern(String),
    Io(io::Error),
    Yaml {
        path: PathBuf,
        source: serde_yaml::Error,
    },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GlobPattern(error) => write!(f, "invalid workflow glob pattern: {error}"),
            Self::Io(error) => write!(f, "failed to read GitHub Actions workflow: {error}"),
            Self::Yaml { path, source } => {
                write!(
                    f,
                    "failed to parse workflow YAML at {}: {source}",
                    path.display()
                )
            }
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Yaml { source, .. } => Some(source),
            Self::GlobPattern(_) => None,
        }
    }
}

#[derive(Debug, serde::Deserialize)]
struct ActionStep {
    uses: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct ActionJob {
    #[serde(default)]
    steps: Vec<ActionStep>,
    uses: Option<String>,
}

#[derive(Debug, serde::Deserialize)]
struct ActionWorkflow {
    jobs: HashMap<String, ActionJob>,
}

#[derive(Debug)]
pub struct GitHubActionsProject {
    workflows: Vec<ActionWorkflow>,
}

impl GitHubActionsProject {
    pub fn from_path(root: &Path) -> Result<Option<Self>, Error> {
        let workflows_path = root.join(".github/workflows");

        if !workflows_path.exists() || !workflows_path.is_dir() {
            return Ok(None);
        }

        let workflows_pattern = workflows_path.to_str().unwrap().to_owned() + "/**";
        let workflows_glob =
            Glob::new(&workflows_pattern).map_err(|e| Error::GlobPattern(e.to_string()))?;

        let mut workflows = Vec::new();

        for entry in workflows_glob.walk("/*.{yml,yaml}") {
            let entry = entry.unwrap();

            println!("Found workflow file: {}", entry.path().display());

            if !entry.file_type().is_file() {
                continue;
            }

            let content: String = std::fs::read_to_string(&entry.path()).map_err(Error::Io)?;
            let action: ActionWorkflow =
                serde_yaml::from_str(&content).map_err(|source| Error::Yaml {
                    path: entry.into_path(),
                    source,
                })?;

            workflows.push(action);
        }

        if workflows.is_empty() {
            return Ok(None);
        }

        Ok(Some(Self { workflows }))
    }
}

pub(super) fn dependency_analysis(
    project: &GitHubActionsProject,
) -> Result<DependencyGroups, super::Error> {
    let mut dependencies: HashMap<String, usize> = HashMap::new();

    for action in &project.workflows {
        action.jobs.values().for_each(|job| {
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

        let project = GitHubActionsProject::from_path(&example_root);

        assert!(project.is_ok());

        let project = project.unwrap();

        assert!(project.is_some());
    }

    #[test]
    fn detects_non_github_actions_project() {
        let example_root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/empty");

        assert!(GitHubActionsProject::from_path(&example_root).is_ok_and(|v| v.is_none()));
    }

    #[test]
    fn analyzes_github_actions_dependencies() {
        let project = GitHubActionsProject {
            workflows: vec![ActionWorkflow {
                jobs: HashMap::from([(
                    "job1".to_string(),
                    ActionJob {
                        steps: vec![ActionStep {
                            uses: Some("actions/checkout@v2".to_string()),
                        }],
                        uses: None,
                    },
                )]),
            }],
        };

        let groups = dependency_analysis(&project).unwrap();
        assert_eq!(groups.iter().count(), 0);
    }
}
