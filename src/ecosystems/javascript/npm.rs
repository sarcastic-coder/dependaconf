use std::{
    collections::{BTreeMap, HashMap},
    fs::read_to_string,
    path::Path,
};

use super::{DependencyMetadata, Error};

#[cfg(test)]
use super::{
    group_dependencies as group_npm_dependencies,
    group_dependencies_by_peer as group_npm_dependencies_by_peer,
    group_dependencies_by_scope as group_npm_dependencies_by_scope, merge_overlapping_peer_groups,
};
#[cfg(test)]
type NpmDependency = DependencyMetadata;

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

pub(super) fn read_dependency_metadata(
    root: &Path,
) -> Result<BTreeMap<String, Vec<DependencyMetadata>>, Error> {
    let contents = read_to_string(root.join("package-lock.json"))?;
    let lockfile: NpmPackageLock = serde_json::from_str(&contents)?;

    if !matches!(lockfile.lockfile_version, 2 | 3) {
        return Err(Error::UnsupportedLockfileVersion(lockfile.lockfile_version));
    }

    if !lockfile.packages.contains_key("") {
        return Err(Error::MissingRootPackageEntry);
    }

    let mut dependencies_by_workspace: BTreeMap<String, Vec<DependencyMetadata>> = lockfile
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
                DependencyMetadata {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_peer_dependencies_from_npm_lockfile() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/npm-peer");

        let dependencies = read_dependency_metadata(&root).unwrap();
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

        let dependencies = read_dependency_metadata(temp_dir.path()).unwrap();
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
