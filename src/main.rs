use std::{collections::HashMap, env, fs::read_to_string, path::Path};

use clap::{Parser, Subcommand};

#[derive(Subcommand)]
enum Commands {
    Write {},
}

#[derive(Parser)]
#[command(version, about, long_about = None)]
struct Cli {}

#[derive(Debug, PartialEq, Eq)]
enum Ecosystem {
    Npm,
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
    #[serde(rename = "peerDependencies", default)]
    peer_dependencies: HashMap<String, String>,
}

#[derive(Debug, PartialEq, Eq)]
struct NpmDependency {
    name: String,
    peer_dependencies: Vec<String>,
}

fn main() -> () {
    let _ = Cli::parse();

    let current_pathbuf = env::current_dir().unwrap();
    let current_path = current_pathbuf.as_path();

    let ecosystem = detect_ecosystem(current_path);

    match ecosystem {
        Some(Ecosystem::Npm) => {
            let dependencies = read_npm_dependency_metadata(current_path).unwrap();
            group_npm_dependencies(&dependencies);
        }
        None => {}
    };
}

fn detect_ecosystem(root: &Path) -> Option<Ecosystem> {
    root.join("package.json")
        .is_file()
        .then_some(Ecosystem::Npm)
}

fn read_npm_dependency_metadata(
    root: &Path,
) -> Result<Vec<NpmDependency>, Box<dyn std::error::Error>> {
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

    let root_package = lockfile.packages.get("").ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "npm package-lock is missing the root package entry",
        )
    })?;

    root_package
        .dependencies
        .keys()
        .map(|name| {
            let package_key = format!("node_modules/{name}");
            let package = lockfile.packages.get(&package_key).ok_or_else(|| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("npm package-lock is missing direct dependency {name}"),
                )
            })?;

            Ok(NpmDependency {
                name: package.name.clone().unwrap_or_else(|| name.clone()),
                peer_dependencies: package.peer_dependencies.keys().cloned().collect(),
            })
        })
        .collect()
}

fn group_npm_dependencies(dependencies: &[NpmDependency]) -> HashMap<String, Vec<String>> {
    let mut groups = group_npm_dependencies_by_peer(dependencies);
    let peer_members: std::collections::HashSet<_> = groups.values().flatten().cloned().collect();

    for (scope, mut members) in group_npm_dependencies_by_scope(dependencies) {
        members.retain(|member| !peer_members.contains(member));
        if !members.is_empty() {
            groups.entry(scope).or_insert_with(Vec::new).extend(members);
        }
    }

    for members in groups.values_mut() {
        members.sort();
        members.dedup();
    }

    groups
}

fn group_npm_dependencies_by_scope(dependencies: &[NpmDependency]) -> HashMap<String, Vec<String>> {
    let mut groups = HashMap::new();

    for dependency in dependencies {
        if let Some((scope, _)) = dependency.name.split_once('/') {
            if scope.starts_with('@') {
                groups
                    .entry(scope.to_string())
                    .or_insert_with(Vec::new)
                    .push(dependency.name.clone());
            }
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
        assert_eq!(detect_ecosystem(&root), Some(Ecosystem::Npm));
    }

    #[test]
    fn does_not_detect_npm_without_package_json() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/empty");
        assert_eq!(detect_ecosystem(&root), None);
    }

    #[test]
    fn reads_peer_dependencies_from_npm_lockfile() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/npm-peer");

        let dependencies = read_npm_dependency_metadata(&root).unwrap();
        let plugin = dependencies
            .iter()
            .find(|dependency| dependency.name == "react-dom")
            .unwrap();

        assert_eq!(plugin.peer_dependencies, vec!["react".to_string()]);
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
        assert_eq!(groups.get("@acme"), Some(&vec!["@acme/other".to_string()]));
    }
}
