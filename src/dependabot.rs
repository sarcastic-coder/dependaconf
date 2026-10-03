use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    io::Write,
    path::Path,
};

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
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
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

fn write_config<W: Write>(
    writer: W,
    groups_by_workspace: &BTreeMap<String, HashMap<String, Vec<String>>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let updates = groups_by_workspace
        .iter()
        .map(|(workspace, groups)| {
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

            Ok(DependabotUpdate {
                package_ecosystem: "npm".to_string(),
                directory: if workspace.is_empty() {
                    "/".to_string()
                } else {
                    format!("/{workspace}")
                },
                schedule: DependabotSchedule {
                    interval: "weekly".to_string(),
                },
                groups: dependabot_groups,
            })
        })
        .collect::<Result<Vec<_>, Box<dyn std::error::Error>>>()?;

    let config = DependabotConfig {
        version: 2,
        updates,
    };
    serde_yaml_ng::to_writer(writer, &config)?;

    Ok(())
}

pub(super) fn write_config_file(
    path: &Path,
    groups_by_workspace: &BTreeMap<String, HashMap<String, Vec<String>>>,
) -> Result<(), Box<dyn std::error::Error>> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    write_config(File::create(path)?, groups_by_workspace)?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct ExpectedGroups {
        acme: ExpectedGroup,
        react: ExpectedGroup,
    }

    #[derive(Debug, PartialEq, serde::Deserialize)]
    struct ExpectedGroup {
        patterns: Vec<String>,
    }

    fn sample_groups() -> BTreeMap<String, HashMap<String, Vec<String>>> {
        BTreeMap::from([(
            String::new(),
            HashMap::from([
                (
                    "@acme".to_string(),
                    vec!["@acme/core".to_string(), "@acme/ui".to_string()],
                ),
                (
                    "react+react-dom".to_string(),
                    vec!["react".to_string(), "react-dom".to_string()],
                ),
            ]),
        )])
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

        write_config(&mut output, &sample_groups()).unwrap();

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
    fn serializes_each_workspace_as_a_separate_update_directory() {
        let groups_by_workspace = BTreeMap::from([
            (
                "packages/client".to_string(),
                HashMap::from([(
                    "graphql".to_string(),
                    vec!["@apollo/client".to_string(), "graphql".to_string()],
                )]),
            ),
            (
                "packages/server".to_string(),
                HashMap::from([(
                    "graphql".to_string(),
                    vec!["@apollo/server".to_string(), "graphql".to_string()],
                )]),
            ),
        ]);
        let mut output = Vec::new();

        write_config(&mut output, &groups_by_workspace).unwrap();

        let contents = String::from_utf8(output).unwrap();
        let config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&contents).unwrap();
        let updates = config["updates"].as_sequence().unwrap();

        assert_eq!(updates.len(), 2);
        assert_eq!(updates[0]["directory"], "/packages/client");
        assert_eq!(
            updates[0]["groups"]["graphql"]["patterns"],
            serde_yaml_ng::Value::Sequence(vec![
                serde_yaml_ng::Value::String("@apollo/client".to_string()),
                serde_yaml_ng::Value::String("graphql".to_string()),
            ])
        );
        assert_eq!(updates[1]["directory"], "/packages/server");
        assert_eq!(
            updates[1]["groups"]["graphql"]["patterns"],
            serde_yaml_ng::Value::Sequence(vec![
                serde_yaml_ng::Value::String("@apollo/server".to_string()),
                serde_yaml_ng::Value::String("graphql".to_string()),
            ])
        );
    }

    #[test]
    fn writes_config_file_and_creates_parent_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join(".github/dependabot.yml");

        write_config_file(&config_path, &sample_groups()).unwrap();

        assert!(config_path.is_file());
    }
}
