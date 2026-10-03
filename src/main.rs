use std::{
    collections::{BTreeMap, HashMap},
    env,
    fs::{File, read_to_string},
    io::Write,
    path::Path,
};

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

    #[serde(rename = "devDependencies", default)]
    dev_dependencies: HashMap<String, String>,

    #[serde(rename = "peerDependencies", default)]
    peer_dependencies: HashMap<String, String>,
}

#[derive(Debug, PartialEq, Eq)]
struct NpmDependency {
    name: String,
    peer_dependencies: Vec<String>,
}

#[derive(serde::Serialize)]
struct DependabotConfig {
    version: u8,
    updates: Vec<DependabotUpdate>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "kebab-case")]
struct DependabotUpdate {
    package_ecosystem: String,
    directory: String,
    schedule: DependabotSchedule,
    groups: BTreeMap<String, DependabotGroup>,
}

#[derive(serde::Serialize)]
struct DependabotSchedule {
    interval: String,
}

#[derive(serde::Serialize)]
struct DependabotGroup {
    patterns: Vec<String>,
}

fn main() -> () {
    let _ = Cli::parse();

    let current_pathbuf = env::current_dir().unwrap();
    let current_path = current_pathbuf.as_path();

    let ecosystem = detect_ecosystem(current_path);

    let groups: Option<HashMap<String, Vec<String>>> = match ecosystem {
        Some(Ecosystem::Npm) => {
            let dependencies = read_npm_dependency_metadata(current_path).unwrap();
            Some(group_npm_dependencies(&dependencies))
        }
        None => None,
    };

    if groups.is_none() {
        return;
    }

    write_dependabot_config_file(Path::new(".github/dependabot.yml"), &groups.unwrap()).unwrap();
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

    let dependencies: HashMap<String, String> = root_package
        .dependencies
        .clone()
        .into_iter()
        .chain(root_package.dev_dependencies.clone())
        .collect();

    dependencies
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
    let mut groups = merge_overlapping_peer_groups(group_npm_dependencies_by_peer(dependencies));
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

fn dependabot_group_identifier(group: &str) -> String {
    let is_scope = group.starts_with('@') && !group.contains('/');
    let name = if is_scope {
        group.trim_start_matches('@').to_string()
    } else {
        common_peer_root(group)
    };
    let slug: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect();

    slug.trim_matches('-').to_string()
}

fn common_peer_root(group: &str) -> String {
    let mut peers = group.split('+');
    let first_peer = peers
        .next()
        .expect("peer groups must contain at least one peer");
    let common_prefix = peers.fold(first_peer.to_string(), |prefix, peer| {
        prefix
            .chars()
            .zip(peer.chars())
            .take_while(|(left, right)| left == right)
            .map(|(character, _)| character)
            .collect()
    });
    let root = common_prefix.trim_end_matches(|character: char| !character.is_ascii_alphanumeric());

    if root.is_empty() {
        group.replace('+', "-")
    } else {
        root.to_string()
    }
}

fn write_dependabot_config<W: Write>(
    writer: W,
    groups: &HashMap<String, Vec<String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let mut dependabot_groups = BTreeMap::new();
    for (group, patterns) in groups {
        let identifier = dependabot_group_identifier(group);
        if dependabot_groups
            .insert(
                identifier,
                DependabotGroup {
                    patterns: patterns.clone(),
                },
            )
            .is_some()
        {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "multiple dependency groups produced the same Dependabot identifier",
            )
            .into());
        }
    }

    let config = DependabotConfig {
        version: 2,
        updates: vec![DependabotUpdate {
            package_ecosystem: "npm".to_string(),
            directory: "/".to_string(),
            schedule: DependabotSchedule {
                interval: "weekly".to_string(),
            },
            groups: dependabot_groups,
        }],
    };
    serde_yaml_ng::to_writer(writer, &config)?;

    Ok(())
}

fn write_dependabot_config_file(
    path: &Path,
    groups: &HashMap<String, Vec<String>>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    write_dependabot_config(File::create(path)?, groups)?;

    Ok(())
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
    mod npm {
        use super::super::*;

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
    }

    mod dependabot_config {
        use super::super::*;

        #[derive(Debug, PartialEq, serde::Deserialize)]
        struct ExpectedGroups {
            acme: ExpectedGroup,
            react: ExpectedGroup,
        }

        #[derive(Debug, PartialEq, serde::Deserialize)]
        struct ExpectedGroup {
            patterns: Vec<String>,
        }

        fn sample_groups() -> HashMap<String, Vec<String>> {
            HashMap::from([
                (
                    "@acme".to_string(),
                    vec!["@acme/core".to_string(), "@acme/ui".to_string()],
                ),
                (
                    "react+react-dom".to_string(),
                    vec!["react".to_string(), "react-dom".to_string()],
                ),
            ])
        }

        #[test]
        fn uses_common_peer_name_root_for_group_identifier() {
            assert_eq!(dependabot_group_identifier("react+react-dom"), "react");
            assert_eq!(dependabot_group_identifier("@acme"), "acme");
        }

        #[test]
        fn uses_all_peer_names_when_group_has_no_common_name_root() {
            assert_eq!(dependabot_group_identifier("react+vite"), "react-vite");
        }

        #[test]
        fn serializes_dependency_groups_to_dependabot_yaml() {
            let mut output = Vec::new();

            write_dependabot_config(&mut output, &sample_groups()).unwrap();

            let contents = String::from_utf8(output).unwrap();
            let config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&contents).unwrap();
            let actual_groups: ExpectedGroups =
                serde_yaml_ng::from_value(config["updates"][0]["groups"].clone()).unwrap();
            let expected_groups = ExpectedGroups {
                acme: ExpectedGroup {
                    patterns: vec!["@acme/core".to_string(), "@acme/ui".to_string()],
                },
                react: ExpectedGroup {
                    patterns: vec!["react".to_string(), "react-dom".to_string()],
                },
            };

            assert_eq!(actual_groups, expected_groups);
        }

        #[test]
        fn writes_dependabot_config_file_and_creates_parent_directory() {
            let temp_dir = tempfile::tempdir().unwrap();
            let config_path = temp_dir.path().join(".github/dependabot.yml");

            write_dependabot_config_file(&config_path, &sample_groups()).unwrap();

            assert!(config_path.is_file());
        }
    }
}
