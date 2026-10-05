use std::{collections::HashMap, error::Error as StdError, io, path::Path};

use super::DependencyGroups;

mod npm;
mod report;
mod yarn;

pub(super) const DEPENDABOT_NAME: &str = "npm";

#[derive(Debug)]
pub(crate) enum Error {
    Io(io::Error),
    Json(serde_json::Error),
    PackageJson(serde_json::Error),
    YarnYaml(serde_yaml_ng::Error),
    InvalidYarnLock(String),
    MissingLockfile,
    UnsupportedLockfileVersion(u32),
    MissingRootPackageEntry,
    MissingDirectDependency(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "failed to read dependency metadata: {error}"),
            Self::Json(error) => write!(f, "failed to parse npm package-lock: {error}"),
            Self::PackageJson(error) => write!(f, "failed to parse package.json: {error}"),
            Self::YarnYaml(error) => write!(f, "failed to parse Yarn lockfile: {error}"),
            Self::InvalidYarnLock(message) => write!(f, "invalid Yarn lockfile: {message}"),
            Self::MissingLockfile => {
                write!(f, "project is missing package-lock.json or yarn.lock")
            }
            Self::UnsupportedLockfileVersion(version) => {
                write!(f, "unsupported npm package-lock version: {version}")
            }
            Self::MissingRootPackageEntry => {
                write!(f, "npm package-lock is missing the root package entry")
            }
            Self::MissingDirectDependency(name) => {
                write!(f, "lockfile is missing direct dependency {name}")
            }
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::PackageJson(error) => Some(error),
            Self::YarnYaml(error) => Some(error),
            Self::InvalidYarnLock(_)
            | Self::MissingLockfile
            | Self::UnsupportedLockfileVersion(_)
            | Self::MissingRootPackageEntry
            | Self::MissingDirectDependency(_) => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_json::Error> for Error {
    fn from(error: serde_json::Error) -> Self {
        Self::Json(error)
    }
}

impl From<serde_yaml_ng::Error> for Error {
    fn from(error: serde_yaml_ng::Error) -> Self {
        Self::YarnYaml(error)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct DependencyMetadata {
    pub(super) name: String,
    pub(super) peer_dependencies: Vec<String>,
}

pub(super) fn is_project(root: &Path) -> bool {
    root.join("package.json").is_file()
}

pub(super) fn dependency_analysis(
    root: &Path,
    include_debug_report: bool,
    combine_workspaces: bool,
) -> Result<(DependencyGroups, Option<String>), super::Error> {
    let dependencies = if root.join("package-lock.json").is_file() {
        npm::read_dependency_metadata(root)?
    } else {
        yarn::read_dependency_metadata(root)?
    };
    let mut groups: DependencyGroups = dependencies
        .iter()
        .map(|(workspace, dependencies)| (workspace.clone(), group_dependencies(dependencies)))
        .collect();
    if combine_workspaces {
        groups = groups.combine_workspaces()?;
    }
    let debug_report = include_debug_report.then(|| {
        if combine_workspaces {
            report::render_combined_groups_report(&groups)
        } else {
            report::render_dependency_report(&dependencies, &groups)
        }
    });

    Ok((groups, debug_report))
}

pub(super) fn group_dependencies(dependencies: &[DependencyMetadata]) -> super::WorkspaceGroups {
    let regular_dependencies: Vec<_> = dependencies
        .iter()
        .filter(|dependency| !dependency.name.starts_with("@types/"))
        .cloned()
        .collect();
    let mut groups =
        merge_overlapping_peer_groups(group_dependencies_by_peer(&regular_dependencies));
    let peer_members: std::collections::HashSet<_> = groups.values().flatten().cloned().collect();

    for (scope, mut members) in group_dependencies_by_scope(&regular_dependencies) {
        members.retain(|member| !peer_members.contains(member));
        if !members.is_empty() {
            groups.entry(scope).or_default().extend(members);
        }
    }

    group_dependencies_by_types(&mut groups, dependencies);

    let mut workspace_groups: super::WorkspaceGroups = groups.into_iter().collect();
    workspace_groups.remove_undersized_groups();

    workspace_groups
}

fn group_dependencies_by_types(
    groups: &mut HashMap<String, Vec<String>>,
    dependencies: &[DependencyMetadata],
) {
    let installed: std::collections::HashSet<_> = dependencies
        .iter()
        .map(|dependency| dependency.name.as_str())
        .collect();
    let mut unmatched_types = Vec::new();

    for dependency in dependencies
        .iter()
        .filter(|dependency| dependency.name.starts_with("@types/"))
    {
        let type_name = dependency.name.trim_start_matches("@types/");
        let implementation = match type_name.split_once("__") {
            Some((scope, name)) => format!("@{scope}/{name}"),
            None => type_name.to_string(),
        };

        if !installed.contains(implementation.as_str()) {
            unmatched_types.push(dependency.name.clone());
            continue;
        }

        let target_group = groups
            .iter()
            .find(|(_, members)| members.iter().any(|member| member == &implementation))
            .map(|(group, _)| group.clone())
            .unwrap_or_else(|| implementation.clone());
        let members = groups
            .entry(target_group)
            .or_insert_with(|| vec![implementation]);
        members.push(dependency.name.clone());
    }

    if !unmatched_types.is_empty() {
        groups
            .entry("@types".to_string())
            .or_default()
            .extend(unmatched_types);
    }
}

pub(super) fn merge_overlapping_peer_groups(
    groups: HashMap<String, Vec<String>>,
) -> HashMap<String, Vec<String>> {
    let mut groups: Vec<_> = groups
        .into_iter()
        .map(|(peer, members)| {
            (
                peer,
                members
                    .into_iter()
                    .collect::<std::collections::HashSet<_>>(),
            )
        })
        .collect();
    groups.sort_by(|left, right| left.0.cmp(&right.0));

    let mut index = 0;
    while index < groups.len() {
        let mut other_index = index + 1;
        while other_index < groups.len() {
            if groups[index].1.is_disjoint(&groups[other_index].1) {
                other_index += 1;
                continue;
            }

            let (other_peer, other_members) = groups.remove(other_index);
            groups[index].0.push('+');
            groups[index].0.push_str(&other_peer);
            groups[index].1.extend(other_members);
            other_index = index + 1;
        }
        index += 1;
    }

    groups
        .into_iter()
        .map(|(peer, members)| (peer, members.into_iter().collect()))
        .collect()
}

pub(super) fn group_dependencies_by_scope(
    dependencies: &[DependencyMetadata],
) -> HashMap<String, Vec<String>> {
    let mut groups = HashMap::new();

    for dependency in dependencies {
        if let Some((scope, _)) = dependency.name.split_once('/')
            && scope.starts_with('@')
        {
            groups
                .entry(scope.to_string())
                .or_insert_with(Vec::new)
                .push(dependency.name.clone());
        }
    }

    groups
}

pub(super) fn group_dependencies_by_peer(
    dependencies: &[DependencyMetadata],
) -> HashMap<String, Vec<String>> {
    let installed: std::collections::HashSet<_> = dependencies
        .iter()
        .map(|dependency| dependency.name.as_str())
        .collect();
    let mut groups = HashMap::new();

    for dependency in dependencies {
        for peer in &dependency.peer_dependencies {
            if installed.contains(peer.as_str()) {
                let group = groups
                    .entry(peer.clone())
                    .or_insert_with(|| vec![peer.clone()]);
                if !group.contains(&dependency.name) {
                    group.push(dependency.name.clone());
                }
            }
        }
    }

    groups
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_project_from_package_manifest() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/npm");
        assert!(is_project(&root));
    }

    #[test]
    fn does_not_detect_project_without_package_manifest() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/empty");
        assert!(!is_project(&root));
    }

    #[test]
    fn reports_missing_lockfile_for_package_manifest() {
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::write(temp_dir.path().join("package.json"), "{}").unwrap();

        assert!(matches!(
            dependency_analysis(temp_dir.path(), false, false),
            Err(super::super::Error::JavaScript(Error::MissingLockfile))
        ));
    }

    #[test]
    fn prefers_npm_lockfile_when_yarn_lockfile_is_also_present() {
        let temp_dir = tempfile::tempdir().unwrap();
        let lockfile = r#"{
            "lockfileVersion": 3,
            "packages": {
                "": {
                    "dependencies": {
                        "npm-package": "^1.0.0"
                    }
                },
                "node_modules/npm-package": {
                    "name": "npm-package"
                }
            }
        }"#;
        std::fs::write(temp_dir.path().join("package.json"), "{}").unwrap();
        std::fs::write(temp_dir.path().join("package-lock.json"), lockfile).unwrap();
        std::fs::write(temp_dir.path().join("yarn.lock"), "not a Yarn lockfile").unwrap();

        let (_, report) = dependency_analysis(temp_dir.path(), true, false).unwrap();

        assert!(report.unwrap().contains("npm-package"));
    }
}
