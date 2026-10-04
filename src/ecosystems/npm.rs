use std::{
    collections::{BTreeMap, HashMap},
    fmt::Write as _,
    fs::read_to_string,
    path::Path,
};

use super::DependencyGroups;

pub(super) const DEPENDABOT_NAME: &str = "npm";

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
) -> Result<(DependencyGroups, Option<String>), Box<dyn std::error::Error>> {
    let dependencies = read_npm_dependency_metadata(root)?;
    let groups = dependencies
        .iter()
        .map(|(workspace, dependencies)| (workspace.clone(), group_npm_dependencies(dependencies)))
        .collect();
    let debug_report =
        include_debug_report.then(|| render_dependency_report(&dependencies, &groups));

    Ok((groups, debug_report))
}

fn render_dependency_report(
    dependencies_by_workspace: &BTreeMap<String, Vec<NpmDependency>>,
    groups_by_workspace: &DependencyGroups,
) -> String {
    let mut report = String::new();

    for (workspace, dependencies) in dependencies_by_workspace {
        let directory = if workspace.is_empty() { "/" } else { workspace };
        let _ = writeln!(report, "Workspace: {directory}");

        let groups = groups_by_workspace
            .get(workspace)
            .into_iter()
            .flat_map(|groups| groups.iter())
            .map(|(group, members)| {
                let mut members = members.clone();
                members.sort();
                (group.as_str(), members)
            })
            .collect::<BTreeMap<_, _>>();
        let assigned = groups
            .values()
            .flatten()
            .map(String::as_str)
            .collect::<std::collections::HashSet<_>>();
        let mut ungrouped = dependencies
            .iter()
            .filter(|dependency| !assigned.contains(dependency.name.as_str()))
            .map(|dependency| dependency.name.clone())
            .collect::<Vec<_>>();
        ungrouped.sort();

        let mut sections = groups
            .into_iter()
            .map(|(group, members)| (format!("Group: {group}"), Some((group, members))))
            .collect::<Vec<_>>();
        if !ungrouped.is_empty() {
            sections.push(("Ungrouped".to_string(), None));
        }

        for (section_index, (label, group)) in sections.iter().enumerate() {
            let is_last_section = section_index + 1 == sections.len();
            let section_branch = if is_last_section {
                "└── "
            } else {
                "├── "
            };
            let child_prefix = if is_last_section { "    " } else { "│   " };
            let _ = writeln!(report, "{section_branch}{label}");

            if let Some((group_name, members)) = group {
                let member_names = members
                    .iter()
                    .map(String::as_str)
                    .collect::<std::collections::HashSet<_>>();
                let peer_targets = dependencies
                    .iter()
                    .filter(|dependency| member_names.contains(dependency.name.as_str()))
                    .flat_map(|dependency| dependency.peer_dependencies.iter())
                    .filter(|peer| member_names.contains(peer.as_str()))
                    .map(String::as_str)
                    .collect::<std::collections::HashSet<_>>();
                let mut roots = members
                    .iter()
                    .filter(|member| !peer_targets.contains(member.as_str()))
                    .collect::<Vec<_>>();
                roots.sort();
                if roots.is_empty() {
                    roots = members.iter().collect();
                    roots.sort();
                }

                let mut visited = std::collections::HashSet::new();
                for (member_index, member) in roots.iter().enumerate() {
                    append_dependency_tree(
                        &mut report,
                        member,
                        dependencies,
                        members,
                        group_name,
                        &child_prefix,
                        member_index + 1 == roots.len(),
                        false,
                        &mut visited,
                    );
                }

                let mut remaining = members
                    .iter()
                    .filter(|member| !visited.contains(member.as_str()))
                    .collect::<Vec<_>>();
                remaining.sort();
                for member in remaining {
                    let is_last_member = visited.len() + 1 == members.len();
                    append_dependency_tree(
                        &mut report,
                        member,
                        dependencies,
                        members,
                        group_name,
                        &child_prefix,
                        is_last_member,
                        false,
                        &mut visited,
                    );
                }
            } else {
                for (member_index, member) in ungrouped.iter().enumerate() {
                    let member_branch = if member_index + 1 == ungrouped.len() {
                        "└── "
                    } else {
                        "├── "
                    };
                    let _ = writeln!(report, "{child_prefix}{member_branch}{member}");
                }
            }
        }
    }

    report
}

#[allow(clippy::too_many_arguments)]
fn append_dependency_tree(
    report: &mut String,
    name: &str,
    dependencies: &[NpmDependency],
    members: &[String],
    group: &str,
    indent: &str,
    is_last: bool,
    is_peer_link: bool,
    visited: &mut std::collections::HashSet<String>,
) {
    let branch = if is_last { "└── " } else { "├── " };
    let _ = write!(report, "{indent}{branch}{name}");
    if is_peer_link {
        let _ = write!(report, " [peer]");
    }
    if !visited.insert(name.to_string()) {
        report.push('\n');
        return;
    }
    if !is_peer_link
        && let Some(dependency) = dependencies
            .iter()
            .find(|dependency| dependency.name == name)
        && let Some(reason) = dependency_group_reason(dependency, group, members)
    {
        let _ = write!(report, "{reason}");
    }
    report.push('\n');

    let Some(dependency) = dependencies
        .iter()
        .find(|dependency| dependency.name == name)
    else {
        return;
    };
    let member_names = members
        .iter()
        .map(String::as_str)
        .collect::<std::collections::HashSet<_>>();
    let mut peers = dependency
        .peer_dependencies
        .iter()
        .filter(|peer| member_names.contains(peer.as_str()))
        .collect::<Vec<_>>();
    peers.sort();

    let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });
    for (peer_index, peer) in peers.iter().enumerate() {
        append_dependency_tree(
            report,
            peer,
            dependencies,
            members,
            group,
            &child_indent,
            peer_index + 1 == peers.len(),
            true,
            visited,
        );
    }
}

fn dependency_group_reason(
    dependency: &NpmDependency,
    group: &str,
    members: &[String],
) -> Option<String> {
    let member_names = members
        .iter()
        .map(String::as_str)
        .collect::<std::collections::HashSet<_>>();
    if dependency.name.starts_with("@types/") {
        let type_name = dependency.name.trim_start_matches("@types/");
        let implementation = match type_name.split_once("__") {
            Some((scope, name)) => format!("@{scope}/{name}"),
            None => type_name.to_string(),
        };
        if member_names.contains(implementation.as_str()) {
            return Some(" [types]".to_string());
        }
    }

    if let Some((scope, _)) = dependency.name.split_once('/')
        && scope.starts_with('@')
        && group == scope
    {
        return Some(format!(" (same npm scope {scope})"));
    }

    None
}

fn read_npm_dependency_metadata(
    root: &Path,
) -> Result<BTreeMap<String, Vec<NpmDependency>>, Box<dyn std::error::Error>> {
    let contents = read_to_string(root.join("package-lock.json"))?;
    let lockfile: NpmPackageLock = serde_json::from_str(&contents)?;

    if !matches!(lockfile.lockfile_version, 2 | 3) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!(
                "unsupported npm package-lock version: {}",
                lockfile.lockfile_version
            ),
        )
        .into());
    }

    if !lockfile.packages.contains_key("") {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "npm package-lock is missing the root package entry",
        )
        .into());
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
                .ok_or_else(|| {
                    std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!("npm package-lock is missing direct dependency {name}"),
                    )
                })?;

            Ok((
                workspace_path.clone(),
                NpmDependency {
                    name: package.name.clone().unwrap_or_else(|| name.clone()),
                    peer_dependencies: package.peer_dependencies.keys().cloned().collect(),
                },
            ))
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?
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

fn group_npm_dependencies(dependencies: &[NpmDependency]) -> HashMap<String, Vec<String>> {
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

    for members in groups.values_mut() {
        members.sort();
        members.dedup();
    }
    groups.retain(|_, members| members.len() > 1);

    groups
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
        assert!(!groups.contains_key("@other/unrelated"));
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
            groups.get("@acme/core"),
            Some(&vec!["@acme/core".to_string(), "@acme/plugin".to_string()])
        );
        assert!(!groups.contains_key("@acme"));
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
            groups.values().next().unwrap(),
            &vec![
                "@testing-library/react".to_string(),
                "react".to_string(),
                "react-dom".to_string(),
                "react-scripts".to_string(),
                "styled-components".to_string(),
            ]
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
            groups.get("react"),
            Some(&vec![
                "@types/react".to_string(),
                "@types/react-dom".to_string(),
                "react".to_string(),
                "react-dom".to_string(),
            ])
        );
        assert_eq!(
            groups.get("express"),
            Some(&vec!["@types/express".to_string(), "express".to_string()])
        );
        assert!(!groups.contains_key("@types"));
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
            groups.get("@acme"),
            Some(&vec![
                "@acme/core".to_string(),
                "@types/acme__core".to_string()
            ])
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

        assert!(groups.values().all(|members| members.len() >= 2));
        assert!(groups.is_empty());
    }

    #[test]
    fn renders_groups_with_peer_dependency_links() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/npm-peer");
        let dependencies = read_npm_dependency_metadata(&root).unwrap();
        let groups = dependencies
            .iter()
            .map(|(workspace, dependencies)| {
                (workspace.clone(), group_npm_dependencies(dependencies))
            })
            .collect();

        let report = render_dependency_report(&dependencies, &groups);

        assert!(report.contains("Workspace: /"));
        assert!(report.contains("Group: react"));
        assert!(report.contains("react-dom"));
        assert!(report.contains("        └── react [peer]"));
        assert!(!report.contains("already shown"));
    }

    #[test]
    fn explains_scope_and_type_definition_group_members() {
        let dependencies = BTreeMap::from([(
            String::new(),
            vec![
                NpmDependency {
                    name: "@acme/core".to_string(),
                    peer_dependencies: vec![],
                },
                NpmDependency {
                    name: "@acme/ui".to_string(),
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
            ],
        )]);
        let groups = dependencies
            .iter()
            .map(|(workspace, dependencies)| {
                (workspace.clone(), group_npm_dependencies(dependencies))
            })
            .collect();

        let report = render_dependency_report(&dependencies, &groups);

        assert!(report.contains("@acme/core (same npm scope @acme)"));
        assert!(report.contains("@types/express [types]"));
    }
}
