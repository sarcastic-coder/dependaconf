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

#[derive(Default)]
pub(super) struct WorkspaceGroups(BTreeMap<String, Vec<String>>);

impl WorkspaceGroups {
    pub(super) fn add_member(&mut self, group: &str, member: &str) {
        self.0
            .entry(group.to_string())
            .or_default()
            .push(member.to_string());
    }

    pub(super) fn normalize(&mut self) {
        for members in self.0.values_mut() {
            members.sort();
            members.dedup();
        }
    }

    pub(super) fn get(&self, group: &str) -> Option<&[String]> {
        self.0.get(group).map(Vec::as_slice)
    }

    pub(super) fn iter(&self) -> impl Iterator<Item = (&str, &[String])> {
        self.0
            .iter()
            .map(|(group, patterns)| (group.as_str(), patterns.as_slice()))
    }

    pub(super) fn len(&self) -> usize {
        self.0.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl FromIterator<(String, Vec<String>)> for WorkspaceGroups {
    fn from_iter<I: IntoIterator<Item = (String, Vec<String>)>>(groups: I) -> Self {
        Self(groups.into_iter().collect())
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

    pub(super) fn len(&self) -> usize {
        self.0.len()
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
    fn returns_unsupported_when_no_supported_ecosystem_is_detected() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/empty");

        assert!(matches!(
            detect(&root, false, false).unwrap(),
            Detection::Unsupported
        ));
    }

    #[test]
    fn workspace_groups_add_and_normalize_members() {
        let mut groups = WorkspaceGroups::default();
        groups.add_member("react", "react-dom");
        groups.add_member("react", "react");
        groups.add_member("react", "react");

        groups.normalize();

        assert_eq!(
            groups.get("react"),
            Some(&["react".to_string(), "react-dom".to_string()][..])
        );
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
