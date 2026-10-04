use std::{
    collections::{BTreeMap, HashMap},
    error::Error as StdError,
    fs::read_to_string,
    io,
    path::Path,
};

use super::DependencyGroups;

mod report;

pub(super) const DEPENDABOT_NAME: &str = "npm";

#[derive(Debug)]
pub(crate) enum Error {
    Io(io::Error),
    Json(serde_json::Error),
    UnsupportedLockfileVersion(u32),
    MissingRootPackageEntry,
    MissingDirectDependency(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "failed to read npm package-lock: {error}"),
            Self::Json(error) => write!(f, "failed to parse npm package-lock: {error}"),
            Self::UnsupportedLockfileVersion(version) => {
                write!(f, "unsupported npm package-lock version: {version}")
            }
            Self::MissingRootPackageEntry => {
                write!(f, "npm package-lock is missing the root package entry")
            }
            Self::MissingDirectDependency(name) => {
                write!(f, "npm package-lock is missing direct dependency {name}")
            }
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Json(error) => Some(error),
            Self::UnsupportedLockfileVersion(_)
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

#[derive(serde::Deserialize)]
struct NpmPackageLock {
    #[serde(rename = "lockfileVersion")]
    lockfile_version: u32,
    #[serde(default)]
    packages: HashMap<String, LockedPackage>,
}

#[derive(serde::Deserialize)]
struct LockedPackage {
    name: Option<String>,

    #[serde(default)]
    dependencies: HashMap<String, String>,

    #[serde(rename = "devDependencies", default)]
    dev_dependencies: HashMap<String, String>,

    #[serde(rename = "peerDependencies", default)]
    peer_dependencies: HashMap<String, String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct NpmDependency {
    name: String,
    peer_dependencies: Vec<String>,
}

pub(super) fn is_project(root: &Path) -> bool {
    root.join("package.json").is_file()
}

pub(super) fn dependency_analysis(
    root: &Path,
    include_debug_report: bool,
    combine_workspaces: bool,
) -> Result<(DependencyGroups, Option<String>), super::Error> {
    let dependencies = read_npm_dependency_metadata(root)?;
    let mut groups: DependencyGroups = dependencies
        .iter()
        .map(|(workspace, dependencies)| (workspace.clone(), group_npm_dependencies(dependencies)))
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

fn read_npm_dependency_metadata(
    root: &Path,
) -> Result<BTreeMap<String, Vec<NpmDependency>>, Error> {
    let contents = read_to_string(root.join("package-lock.json"))?;
    let lockfile: NpmPackageLock = serde_json::from_str(&contents)?;

    if !matches!(lockfile.lockfile_version, 2 | 3) {
        return Err(Error::UnsupportedLockfileVersion(lockfile.lockfile_version));
    }

    if !lockfile.packages.contains_key("") {
        return Err(Error::MissingRootPackageEntry);
    }

    let mut dependencies_by_workspace: BTreeMap<String, Vec<NpmDependency>> = lockfile
        .packages
        .iter()
        .filter(|(path, _)| !path.split('/').any(|component| component == "node_modules"))
        .flat_map(|(workspace_path, workspace)| {
            workspace
                .dependencies
                .keys()
                .chain(workspace.dev_dependencies.keys())
                .map(move |name| (workspace_path, name))
        })
        .map(|(workspace_path, name)| {
            let package = find_npm_dependency_package(&lockfile.packages, workspace_path, name)
                .ok_or_else(|| Error::MissingDirectDependency(name.clone()))?;

            Ok((
                workspace_path.clone(),
                NpmDependency {
                    name: package.name.clone().unwrap_or_else(|| name.clone()),
                    peer_dependencies: package.peer_dependencies.keys().cloned().collect(),
                },
            ))
        })
        .collect::<Result<Vec<_>, Error>>()?
        .into_iter()
        .fold(BTreeMap::new(), |mut grouped, (workspace, dependency)| {
            grouped.entry(workspace).or_default().push(dependency);
            grouped
        });

    for workspace_path in lockfile
        .packages
        .keys()
        .filter(|path| !path.split('/').any(|component| component == "node_modules"))
    {
        dependencies_by_workspace
            .entry(workspace_path.clone())
            .or_default();
    }

    Ok(dependencies_by_workspace)
}

fn find_npm_dependency_package<'a>(
    packages: &'a HashMap<String, LockedPackage>,
    workspace_path: &str,
    dependency_name: &str,
) -> Option<&'a LockedPackage> {
    let mut directory = workspace_path;
    loop {
        let package_path = if directory.is_empty() {
            format!("node_modules/{dependency_name}")
        } else {
            format!("{directory}/node_modules/{dependency_name}")
        };
        if let Some(package) = packages.get(&package_path) {
            return Some(package);
        }

        if let Some((parent, _)) = directory.rsplit_once('/') {
            directory = parent;
        } else if !directory.is_empty() {
            directory = "";
        } else {
            return None;
        }
    }
}

fn group_npm_dependencies(dependencies: &[NpmDependency]) -> super::WorkspaceGroups {
    let regular_dependencies: Vec<_> = dependencies
        .iter()
        .filter(|dependency| !dependency.name.starts_with("@types/"))
        .cloned()
        .collect();
    let mut groups =
        merge_overlapping_peer_groups(group_npm_dependencies_by_peer(&regular_dependencies));
    let peer_members: std::collections::HashSet<_> = groups.values().flatten().cloned().collect();

    for (scope, mut members) in group_npm_dependencies_by_scope(&regular_dependencies) {
        members.retain(|member| !peer_members.contains(member));
        if !members.is_empty() {
            groups.entry(scope).or_default().extend(members);
        }
    }

    group_npm_dependencies_by_types(&mut groups, dependencies);

    let mut workspace_groups: super::WorkspaceGroups = groups.into_iter().collect();
    workspace_groups.remove_undersized_groups();

    workspace_groups
}

fn group_npm_dependencies_by_types(
    groups: &mut HashMap<String, Vec<String>>,
    dependencies: &[NpmDependency],
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

fn merge_overlapping_peer_groups(
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

fn group_npm_dependencies_by_scope(dependencies: &[NpmDependency]) -> HashMap<String, Vec<String>> {
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

fn group_npm_dependencies_by_peer(dependencies: &[NpmDependency]) -> HashMap<String, Vec<String>> {
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
    fn can_detect_npm_ecosystem() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/npm");
        assert!(is_project(&root));
    }

    #[test]
    fn does_not_detect_npm_without_package_json() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/empty");
        assert!(!is_project(&root));
    }

    #[test]
    fn reads_peer_dependencies_from_npm_lockfile() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/npm-peer");

        let dependencies = read_npm_dependency_metadata(&root).unwrap();
        let plugin = dependencies
            .get("")
            .unwrap()
            .iter()
            .find(|dependency| dependency.name == "react-dom")
            .unwrap();

        assert_eq!(plugin.peer_dependencies, vec!["react".to_string()]);
    }

    #[test]
    fn reads_direct_dependencies_from_npm_workspace_packages() {
        let temp_dir = tempfile::tempdir().unwrap();
        let lockfile = r#"{
            "lockfileVersion": 3,
            "packages": {
                "": {},
                "packages/server": {
                    "dependencies": {
                        "@apollo/server": "^5.0.0",
                        "graphql": "^16.0.0"
                    }
                },
                "packages/client": {
                    "dependencies": {
                        "@apollo/client": "^4.0.0",
                        "graphql": "^16.0.0"
                    }
                },
                "node_modules/@apollo/server": {
                    "peerDependencies": {
                        "graphql": "^16.0.0"
                    }
                },
                "node_modules/@apollo/client": {
                    "peerDependencies": {
                        "graphql": "^16.0.0"
                    }
                },
                "node_modules/graphql": {}
            }
        }"#;
        std::fs::write(temp_dir.path().join("package-lock.json"), lockfile).unwrap();

        let dependencies = read_npm_dependency_metadata(temp_dir.path()).unwrap();
        assert!(dependencies.get("").unwrap().is_empty());
        assert_eq!(
            dependencies
                .get("packages/client")
                .unwrap()
                .iter()
                .map(|dependency| dependency.name.as_str())
                .collect::<std::collections::HashSet<_>>(),
            std::collections::HashSet::from(["@apollo/client", "graphql"])
        );
        assert_eq!(
            dependencies
                .get("packages/server")
                .unwrap()
                .iter()
                .map(|dependency| dependency.name.as_str())
                .collect::<std::collections::HashSet<_>>(),
            std::collections::HashSet::from(["@apollo/server", "graphql"])
        );
        for workspace in ["packages/client", "packages/server"] {
            assert!(
                dependencies
                    .get(workspace)
                    .unwrap()
                    .iter()
                    .filter(|dependency| dependency.name.starts_with("@apollo/"))
                    .all(|dependency| dependency.peer_dependencies == vec!["graphql"])
            );
        }
    }

    #[test]
    fn groups_npm_packages_by_scope() {
        let dependencies = vec![
            NpmDependency {
                name: "@acme/core".to_string(),
                peer_dependencies: vec![],
            },
            NpmDependency {
                name: "@acme/ui".to_string(),
                peer_dependencies: vec![],
            },
            NpmDependency {
                name: "@other/plugin".to_string(),
                peer_dependencies: vec![],
            },
        ];

        let groups = group_npm_dependencies_by_scope(&dependencies);

        assert_eq!(
            groups,
            HashMap::from([
                (
                    "@acme".to_string(),
                    vec!["@acme/core".to_string(), "@acme/ui".to_string()],
                ),
                ("@other".to_string(), vec!["@other/plugin".to_string()],),
            ])
        );
    }

    #[test]
    fn groups_npm_dependency_with_its_installed_peer() {
        let dependencies = vec![
            NpmDependency {
                name: "@npm/tea-latte".to_string(),
                peer_dependencies: vec!["@npm/tea".to_string()],
            },
            NpmDependency {
                name: "@npm/tea".to_string(),
                peer_dependencies: vec![],
            },
            NpmDependency {
                name: "@other/unrelated".to_string(),
                peer_dependencies: vec![],
            },
        ];

        let groups = group_npm_dependencies_by_peer(&dependencies);

        assert_eq!(
            groups.get("@npm/tea"),
            Some(&vec!["@npm/tea".to_string(), "@npm/tea-latte".to_string()])
        );
        assert!(groups.get("@other/unrelated").is_none());
    }

    #[test]
    fn peer_group_takes_precedence_over_scope_group() {
        let dependencies = vec![
            NpmDependency {
                name: "@acme/core".to_string(),
                peer_dependencies: vec![],
            },
            NpmDependency {
                name: "@acme/plugin".to_string(),
                peer_dependencies: vec!["@acme/core".to_string()],
            },
            NpmDependency {
                name: "@acme/other".to_string(),
                peer_dependencies: vec![],
            },
        ];

        let groups = group_npm_dependencies(&dependencies);

        assert_eq!(
            groups
                .iter()
                .find(|(group, _)| *group == "@acme/core")
                .map(|(_, members)| members),
            Some(&["@acme/core".to_string(), "@acme/plugin".to_string()][..])
        );
        assert!(groups.iter().all(|(group, _)| group != "@acme"));
    }

    #[test]
    fn merges_overlapping_peer_groups_without_repeating_dependencies() {
        let dependencies = vec![
            NpmDependency {
                name: "react".to_string(),
                peer_dependencies: vec![],
            },
            NpmDependency {
                name: "react-dom".to_string(),
                peer_dependencies: vec!["react".to_string()],
            },
            NpmDependency {
                name: "@testing-library/react".to_string(),
                peer_dependencies: vec!["react".to_string(), "react-dom".to_string()],
            },
            NpmDependency {
                name: "react-scripts".to_string(),
                peer_dependencies: vec!["react".to_string(), "react-dom".to_string()],
            },
            NpmDependency {
                name: "styled-components".to_string(),
                peer_dependencies: vec!["react".to_string(), "react-dom".to_string()],
            },
        ];

        let groups = group_npm_dependencies(&dependencies);

        assert_eq!(groups.len(), 1);
        assert_eq!(
            groups.iter().next().unwrap().1,
            &[
                "@testing-library/react".to_string(),
                "react".to_string(),
                "react-dom".to_string(),
                "react-scripts".to_string(),
                "styled-components".to_string(),
            ][..]
        );
    }

    #[test]
    fn merges_peer_groups_when_an_earlier_merge_creates_an_overlap() {
        let groups = HashMap::from([
            (
                "typescript".to_string(),
                vec!["typescript".to_string(), "vue".to_string()],
            ),
            (
                "vite".to_string(),
                vec!["vite".to_string(), "@vitejs/plugin-vue".to_string()],
            ),
            (
                "vue".to_string(),
                vec![
                    "vue".to_string(),
                    "vue-router".to_string(),
                    "@vitejs/plugin-vue".to_string(),
                ],
            ),
        ]);

        let merged = merge_overlapping_peer_groups(groups);

        assert_eq!(merged.len(), 1);
        let mut members = merged.values().next().unwrap().clone();
        members.sort();
        assert_eq!(
            members,
            vec![
                "@vitejs/plugin-vue".to_string(),
                "typescript".to_string(),
                "vite".to_string(),
                "vue".to_string(),
                "vue-router".to_string(),
            ]
        );
    }

    #[test]
    fn groups_types_with_their_implementation_packages() {
        let dependencies = vec![
            NpmDependency {
                name: "react".to_string(),
                peer_dependencies: vec![],
            },
            NpmDependency {
                name: "react-dom".to_string(),
                peer_dependencies: vec!["react".to_string()],
            },
            NpmDependency {
                name: "@types/react".to_string(),
                peer_dependencies: vec![],
            },
            NpmDependency {
                name: "@types/react-dom".to_string(),
                peer_dependencies: vec![],
            },
            NpmDependency {
                name: "express".to_string(),
                peer_dependencies: vec![],
            },
            NpmDependency {
                name: "@types/express".to_string(),
                peer_dependencies: vec![],
            },
            NpmDependency {
                name: "@types/node".to_string(),
                peer_dependencies: vec![],
            },
        ];

        let groups = group_npm_dependencies(&dependencies);

        assert_eq!(
            groups
                .iter()
                .find(|(group, _)| *group == "react")
                .map(|(_, members)| members),
            Some(
                &[
                    "@types/react".to_string(),
                    "@types/react-dom".to_string(),
                    "react".to_string(),
                    "react-dom".to_string(),
                ][..]
            )
        );
        assert_eq!(
            groups
                .iter()
                .find(|(group, _)| *group == "express")
                .map(|(_, members)| members),
            Some(&["@types/express".to_string(), "express".to_string()][..])
        );
        assert!(groups.iter().all(|(group, _)| group != "@types"));
    }

    #[test]
    fn maps_scoped_types_packages_to_scoped_implementations() {
        let dependencies = vec![
            NpmDependency {
                name: "@acme/core".to_string(),
                peer_dependencies: vec![],
            },
            NpmDependency {
                name: "@types/acme__core".to_string(),
                peer_dependencies: vec![],
            },
        ];

        let groups = group_npm_dependencies(&dependencies);

        assert_eq!(
            groups
                .iter()
                .find(|(group, _)| *group == "@acme")
                .map(|(_, members)| members),
            Some(&["@acme/core".to_string(), "@types/acme__core".to_string()][..])
        );
    }

    #[test]
    fn does_not_create_groups_with_only_one_member() {
        let dependencies = vec![
            NpmDependency {
                name: "@acme/standalone".to_string(),
                peer_dependencies: vec![],
            },
            NpmDependency {
                name: "@types/node".to_string(),
                peer_dependencies: vec![],
            },
        ];

        let groups = group_npm_dependencies(&dependencies);

        assert!(groups.iter().all(|(_, members)| members.len() >= 2));
        assert!(groups.iter().next().is_none());
    }
}
