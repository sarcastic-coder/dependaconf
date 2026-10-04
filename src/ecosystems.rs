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
    SharedDependenciesGroupConflict,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Npm(error) => write!(f, "npm dependency analysis failed: {error}"),
            Self::SharedDependenciesGroupConflict => write!(
                f,
                "a dependency group conflicts with the shared-dependencies group"
            ),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Npm(error) => Some(error),
            Self::SharedDependenciesGroupConflict => None,
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

pub(super) fn detect(
    root: &Path,
    include_debug_report: bool,
    combine_workspaces: bool,
) -> Result<Detection, Error> {
    if !npm::is_project(root) {
        return Ok(Detection::Unsupported);
    }

    let (groups_by_workspace, debug_report) =
        npm::dependency_analysis(root, include_debug_report, combine_workspaces)?;

    Ok(Detection::Detected(ProjectDependencies {
        package_ecosystem: npm::DEPENDABOT_NAME,
        groups_by_workspace,
        debug_report,
    }))
}

pub(super) fn combine_workspace_groups(
    groups_by_workspace: &DependencyGroups,
) -> Result<BTreeMap<String, Vec<String>>, Error> {
    let mut workspaces_by_pattern = HashMap::<String, std::collections::HashSet<String>>::new();
    for (workspace, groups) in groups_by_workspace {
        for patterns in groups.values() {
            for pattern in patterns {
                workspaces_by_pattern
                    .entry(pattern.clone())
                    .or_default()
                    .insert(workspace.clone());
            }
        }
    }
    let shared_patterns = workspaces_by_pattern
        .into_iter()
        .filter_map(|(pattern, workspaces)| (workspaces.len() > 1).then_some(pattern))
        .collect::<std::collections::HashSet<_>>();
    let mut combined_groups = BTreeMap::<String, Vec<String>>::new();
    for groups in groups_by_workspace.values() {
        for (group, patterns) in groups {
            combined_groups.entry(group.clone()).or_default().extend(
                patterns
                    .iter()
                    .filter(|pattern| !shared_patterns.contains(*pattern))
                    .cloned(),
            );
        }
    }
    if !shared_patterns.is_empty() {
        if combined_groups.contains_key("shared-dependencies") {
            return Err(Error::SharedDependenciesGroupConflict);
        }
        combined_groups.insert(
            "shared-dependencies".to_string(),
            shared_patterns.into_iter().collect(),
        );
    }
    let mut seen_patterns = std::collections::HashSet::new();
    combined_groups.retain(|_, patterns| {
        patterns.sort();
        patterns.dedup();
        patterns.retain(|pattern| seen_patterns.insert(pattern.clone()));
        !patterns.is_empty()
    });
    Ok(combined_groups)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_project_and_returns_ecosystem_neutral_groups() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/npm-peer");

        let Detection::Detected(project) = detect(&root, false, false).unwrap() else {
            panic!("expected project to be detected");
        };

        assert_eq!(project.package_ecosystem, "npm");
        assert!(project.groups_by_workspace.contains_key(""));
    }

    #[test]
    fn returns_unsupported_when_no_supported_ecosystem_is_detected() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/empty");

        assert!(matches!(
            detect(&root, false, false).unwrap(),
            Detection::Unsupported
        ));
    }

    #[test]
    fn combines_groups_during_detection() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/apollo-monorepo");

        let Detection::Detected(project) = detect(&root, true, true).unwrap() else {
            panic!("expected project to be detected");
        };

        assert_eq!(project.groups_by_workspace.len(), 1);
        let groups = project.groups_by_workspace.get("").unwrap();
        assert_eq!(
            groups.get("shared-dependencies").unwrap(),
            &vec!["graphql".to_string()]
        );
        assert!(
            project
                .debug_report
                .as_deref()
                .unwrap()
                .contains("Group: shared-dependencies")
        );
    }

    #[test]
    fn reports_conflict_when_combined_shared_group_name_is_already_used() {
        let groups = BTreeMap::from([
            (
                "packages/client".to_string(),
                HashMap::from([(
                    "shared-dependencies".to_string(),
                    vec!["graphql".to_string(), "@apollo/client".to_string()],
                )]),
            ),
            (
                "packages/server".to_string(),
                HashMap::from([(
                    "graphql".to_string(),
                    vec!["graphql".to_string(), "@apollo/server".to_string()],
                )]),
            ),
        ]);

        assert!(matches!(
            combine_workspace_groups(&groups),
            Err(Error::SharedDependenciesGroupConflict)
        ));
    }
}
