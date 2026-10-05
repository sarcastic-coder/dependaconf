use std::{
    collections::{BTreeMap, HashMap},
    error::Error as StdError,
    fmt,
    path::Path,
};

mod javascript;

#[derive(Debug)]
pub(super) enum Error {
    JavaScript(javascript::Error),
    SharedDependenciesGroupConflict,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::JavaScript(error) => write!(f, "JavaScript dependency analysis failed: {error}"),
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
            Self::JavaScript(error) => Some(error),
            Self::SharedDependenciesGroupConflict => None,
        }
    }
}

impl From<javascript::Error> for Error {
    fn from(error: javascript::Error) -> Self {
        Self::JavaScript(error)
    }
}

#[derive(Default)]
pub(super) struct WorkspaceGroups(BTreeMap<String, Vec<String>>);

impl WorkspaceGroups {
    fn normalize(&mut self) {
        for members in self.0.values_mut() {
            members.sort();
            members.dedup();
        }
    }

    pub(super) fn remove_undersized_groups(&mut self) {
        self.0.retain(|_, members| members.len() > 1);
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.0
            .iter()
            .map(|(group, patterns)| (group.as_str(), patterns.as_slice()))
    }

    pub(super) fn len(&self) -> usize {
        self.0.len()
    }
}

impl FromIterator<(String, Vec<String>)> for WorkspaceGroups {
    fn from_iter<I: IntoIterator<Item = (String, Vec<String>)>>(groups: I) -> Self {
        let mut workspace_groups = Self(groups.into_iter().collect());
        workspace_groups.normalize();
        workspace_groups
    }
}

#[derive(Default)]
pub(super) struct DependencyGroups(BTreeMap<String, WorkspaceGroups>);

impl DependencyGroups {
    pub(super) fn get(&self, workspace: &str) -> Option<&WorkspaceGroups> {
        self.0.get(workspace)
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&str, &WorkspaceGroups)> {
        self.0
            .iter()
            .map(|(workspace, groups)| (workspace.as_str(), groups))
    }

    pub(super) fn combine_workspaces(&self) -> Result<Self, Error> {
        let mut workspaces_by_pattern = HashMap::<String, std::collections::HashSet<String>>::new();
        for (workspace, groups) in self.iter() {
            for (_, patterns) in groups.iter() {
                for pattern in patterns {
                    workspaces_by_pattern
                        .entry(pattern.clone())
                        .or_default()
                        .insert(workspace.to_string());
                }
            }
        }
        let shared_patterns = workspaces_by_pattern
            .into_iter()
            .filter_map(|(pattern, workspaces)| (workspaces.len() > 1).then_some(pattern))
            .collect::<std::collections::HashSet<_>>();
        let mut combined_groups = BTreeMap::<String, Vec<String>>::new();
        for (_, groups) in self.iter() {
            for (group, patterns) in groups.iter() {
                combined_groups
                    .entry(group.to_string())
                    .or_default()
                    .extend(
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

        Ok([(String::new(), combined_groups.into_iter().collect())]
            .into_iter()
            .collect())
    }
}

impl FromIterator<(String, WorkspaceGroups)> for DependencyGroups {
    fn from_iter<I: IntoIterator<Item = (String, WorkspaceGroups)>>(workspaces: I) -> Self {
        Self(workspaces.into_iter().collect())
    }
}

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
    if !javascript::is_project(root) {
        return Ok(Detection::Unsupported);
    }

    let (groups_by_workspace, debug_report) =
        javascript::dependency_analysis(root, include_debug_report, combine_workspaces)?;

    Ok(Detection::Detected(ProjectDependencies {
        package_ecosystem: javascript::DEPENDABOT_NAME,
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

        let Detection::Detected(project) = detect(&root, false, false).unwrap() else {
            panic!("expected project to be detected");
        };

        assert_eq!(project.package_ecosystem, "npm");
        assert!(project.groups_by_workspace.get("").is_some());
    }

    #[test]
    fn detects_yarn_lockfile_as_dependabot_npm_ecosystem() {
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            temp_dir.path().join("package.json"),
            r#"{"dependencies":{"react":"^18.0.0"}}"#,
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("yarn.lock"),
            r#"
# yarn lockfile v1
react@^18.0.0:
  version "18.2.0"
"#,
        )
        .unwrap();

        let Detection::Detected(project) = detect(temp_dir.path(), false, false).unwrap() else {
            panic!("expected Yarn project to be detected");
        };

        assert_eq!(project.package_ecosystem, "npm");
        assert!(project.groups_by_workspace.get("").is_some());
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
    fn workspace_groups_collect_members_sorted_and_unique() {
        let groups: WorkspaceGroups = [
            (
                "react".to_string(),
                vec![
                    "react-dom".to_string(),
                    "react".to_string(),
                    "react".to_string(),
                ],
            ),
            (
                "react-only".to_string(),
                vec!["react".to_string(), "react".to_string()],
            ),
        ]
        .into_iter()
        .collect();

        assert_eq!(
            groups
                .iter()
                .find(|(group, _)| *group == "react")
                .map(|(_, members)| members),
            Some(&["react".to_string(), "react-dom".to_string()][..])
        );
        assert_eq!(
            groups
                .iter()
                .find(|(group, _)| *group == "react-only")
                .map(|(_, members)| members),
            Some(&["react".to_string()][..])
        );
    }

    #[test]
    fn combines_groups_during_detection() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/npm/apollo-monorepo");

        let Detection::Detected(project) = detect(&root, true, true).unwrap() else {
            panic!("expected project to be detected");
        };

        assert_eq!(project.groups_by_workspace.iter().count(), 1);
        let groups = project.groups_by_workspace.get("").unwrap();
        assert_eq!(
            groups
                .iter()
                .find(|(group, _)| *group == "shared-dependencies")
                .unwrap()
                .1,
            &["graphql".to_string()][..]
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
        let groups: DependencyGroups = [
            (
                "packages/client".to_string(),
                [(
                    "shared-dependencies".to_string(),
                    vec!["graphql".to_string(), "@apollo/client".to_string()],
                )]
                .into_iter()
                .collect(),
            ),
            (
                "packages/server".to_string(),
                [(
                    "graphql".to_string(),
                    vec!["graphql".to_string(), "@apollo/server".to_string()],
                )]
                .into_iter()
                .collect(),
            ),
        ]
        .into_iter()
        .collect();

        assert!(matches!(
            groups.combine_workspaces(),
            Err(Error::SharedDependenciesGroupConflict)
        ));
    }
}
