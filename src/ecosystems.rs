use std::{
    collections::{BTreeMap, HashMap},
    error::Error as StdError,
    fmt,
    path::Path,
};

mod npm;

#[derive(Debug)]
pub(super) enum Error {
    Npm(npm::Error),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Npm(error) => write!(f, "npm dependency analysis failed: {error}"),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Npm(error) => Some(error),
        }
    }
}

impl From<npm::Error> for Error {
    fn from(error: npm::Error) -> Self {
        Self::Npm(error)
    }
}

pub(super) type DependencyGroups = BTreeMap<String, HashMap<String, Vec<String>>>;

pub(super) struct ProjectDependencies {
    pub(super) package_ecosystem: &'static str,
    pub(super) groups_by_workspace: DependencyGroups,
    pub(super) debug_report: Option<String>,
}

pub(super) enum Detection {
    Unsupported,
    Detected(ProjectDependencies),
}

pub(super) fn detect(root: &Path, include_debug_report: bool) -> Result<Detection, Error> {
    if !npm::is_project(root) {
        return Ok(Detection::Unsupported);
    }

    let (groups_by_workspace, debug_report) = npm::dependency_analysis(root, include_debug_report)?;

    Ok(Detection::Detected(ProjectDependencies {
        package_ecosystem: npm::DEPENDABOT_NAME,
        groups_by_workspace,
        debug_report,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_project_and_returns_ecosystem_neutral_groups() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/npm-peer");

        let Detection::Detected(project) = detect(&root, false).unwrap() else {
            panic!("expected project to be detected");
        };

        assert_eq!(project.package_ecosystem, "npm");
        assert!(project.groups_by_workspace.contains_key(""));
    }

    #[test]
    fn returns_unsupported_when_no_supported_ecosystem_is_detected() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/empty");

        assert!(matches!(
            detect(&root, false).unwrap(),
            Detection::Unsupported
        ));
    }
}
