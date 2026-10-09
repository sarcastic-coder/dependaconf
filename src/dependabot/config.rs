use std::{
    collections::BTreeMap,
    error::Error as StdError,
    fs::File,
    io::{self, Write},
    path::Path,
    str::FromStr,
};

use crate::ecosystems::DependencyGroups;
use yaml_edit::YamlFile;

use super::grouping::{
    ConfigIndentation, dependabot_group_identifier, detect_config_indentation, yaml_scalar,
};
use super::merge::merge_generated_groups_losslessly;
use super::model::{DependabotCooldown, DependabotGroup, DependabotSchedule, DependabotUpdate};

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Yaml(serde_yaml_ng::Error),
    YamlEdit(yaml_edit::YamlError),
    Utf8(std::string::FromUtf8Error),
    DuplicateGroupIdentifier,
    InvalidConfig(&'static str),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Io(error) => write!(f, "Dependabot config I/O failed: {error}"),
            Self::Yaml(error) => write!(f, "Dependabot config YAML failed: {error}"),
            Self::YamlEdit(error) => write!(f, "Dependabot config YAML editing failed: {error}"),
            Self::Utf8(error) => write!(f, "Dependabot config UTF-8 conversion failed: {error}"),
            Self::DuplicateGroupIdentifier => write!(
                f,
                "multiple dependency groups produced the same Dependabot identifier"
            ),
            Self::InvalidConfig(message) => f.write_str(message),
        }
    }
}

impl StdError for Error {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        match self {
            Self::Io(error) => Some(error),
            Self::Yaml(error) => Some(error),
            Self::YamlEdit(error) => Some(error),
            Self::Utf8(error) => Some(error),
            Self::DuplicateGroupIdentifier | Self::InvalidConfig(_) => None,
        }
    }
}

impl From<io::Error> for Error {
    fn from(error: io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<serde_yaml_ng::Error> for Error {
    fn from(error: serde_yaml_ng::Error) -> Self {
        Self::Yaml(error)
    }
}

impl From<yaml_edit::YamlError> for Error {
    fn from(error: yaml_edit::YamlError) -> Self {
        Self::YamlEdit(error)
    }
}

impl From<std::string::FromUtf8Error> for Error {
    fn from(error: std::string::FromUtf8Error) -> Self {
        Self::Utf8(error)
    }
}

fn write_config_with_indentation<W: Write>(
    mut writer: W,
    package_ecosystem: &str,
    groups_by_workspace: &DependencyGroups,
    indentation: ConfigIndentation,
) -> Result<(), Error> {
    let updates = groups_by_workspace
        .iter()
        .map(|(workspace, groups)| {
            let mut dependabot_groups = BTreeMap::new();
            for (group, patterns) in groups.iter() {
                let identifier = dependabot_group_identifier(group);
                if dependabot_groups
                    .insert(
                        identifier,
                        DependabotGroup {
                            patterns: patterns.to_vec(),
                        },
                    )
                    .is_some()
                {
                    return Err(Error::DuplicateGroupIdentifier);
                }
            }

            Ok(DependabotUpdate {
                package_ecosystem: package_ecosystem.to_string(),
                directory: if workspace.is_empty() {
                    "/".to_string()
                } else {
                    format!("/{workspace}")
                },
                cooldown: Some(DependabotCooldown {
                    semver_major_days: 30,
                    semver_minor_days: 7,
                    semver_patch_days: 3,
                }),
                schedule: DependabotSchedule {
                    interval: "weekly".to_string(),
                },
                groups: if dependabot_groups.is_empty() {
                    None
                } else {
                    Some(dependabot_groups)
                },
            })
        })
        .collect::<Result<Vec<_>, Error>>()?;

    writeln!(writer, "version: 2")?;
    writeln!(writer, "updates:")?;

    for update in updates {
        let update_indent = indentation.update_item;
        let field_indent = update_indent + 2;
        writeln!(
            writer,
            "{}- package-ecosystem: {}",
            " ".repeat(update_indent),
            yaml_scalar(&update.package_ecosystem)
        )?;
        writeln!(
            writer,
            "{}directory: {}",
            " ".repeat(field_indent),
            yaml_scalar(&update.directory)
        )?;
        writeln!(writer, "{}schedule:", " ".repeat(field_indent))?;
        writeln!(
            writer,
            "{}interval: {}",
            " ".repeat(field_indent + 2),
            yaml_scalar(&update.schedule.interval)
        )?;

        if let Some(cooldown) = update.cooldown {
            writeln!(writer, "{}cooldown:", " ".repeat(field_indent))?;
            writeln!(
                writer,
                "{}semver-major-days: {}",
                " ".repeat(field_indent + 2),
                cooldown.semver_major_days
            )?;
            writeln!(
                writer,
                "{}semver-minor-days: {}",
                " ".repeat(field_indent + 2),
                cooldown.semver_minor_days
            )?;
            writeln!(
                writer,
                "{}semver-patch-days: {}",
                " ".repeat(field_indent + 2),
                cooldown.semver_patch_days
            )?;
        }

        if let Some(groups) = update.groups {
            writeln!(writer, "{}groups:", " ".repeat(field_indent))?;
            for (group_name, group) in groups {
                let group_indent = field_indent + 2;
                let patterns_indent = group_indent + 2;
                writeln!(writer, "{}{group_name}:", " ".repeat(group_indent))?;
                writeln!(writer, "{}patterns:", " ".repeat(patterns_indent))?;
                for pattern in group.patterns {
                    writeln!(
                        writer,
                        "{}- {}",
                        " ".repeat(patterns_indent + indentation.pattern_item_offset),
                        yaml_scalar(&pattern)
                    )?;
                }
            }
        }
    }

    Ok(())
}

pub fn write_config_file(
    path: &Path,
    package_ecosystem: &str,
    groups_by_workspace: &DependencyGroups,
) -> Result<(), Error> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        std::fs::create_dir_all(parent)?;
    }
    let existing_contents = if path.exists() {
        Some(std::fs::read_to_string(path)?)
    } else {
        None
    };
    let indentation = existing_contents
        .as_deref()
        .map(detect_config_indentation)
        .unwrap_or_default();
    let mut generated = Vec::new();
    write_config_with_indentation(
        &mut generated,
        package_ecosystem,
        groups_by_workspace,
        indentation,
    )?;
    let generated_text = String::from_utf8(generated)?;

    let contents = if let Some(contents) = existing_contents {
        let _: serde_yaml_ng::Value = serde_yaml_ng::from_str(&contents)?;

        let existing_yaml = YamlFile::from_str(&contents)?;
        if existing_yaml.document().is_none() {
            let mut output = contents;
            if !output.is_empty() && !output.ends_with('\n') {
                output.push('\n');
            }
            output.push_str(&generated_text);
            output
        } else {
            let generated_yaml = YamlFile::from_str(&generated_text)?;
            merge_generated_groups_losslessly(&existing_yaml, &generated_yaml)?;
            existing_yaml.to_string()
        }
    } else {
        generated_text
    };

    let mut file = File::create(path)?;
    file.write_all(contents.as_bytes())?;
    file.flush()?;

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::super::merge::{dependency_pattern_matches, groups_match_by_dependencies};
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

    fn write_config<W: Write>(
        writer: W,
        package_ecosystem: &str,
        groups_by_workspace: &DependencyGroups,
    ) -> Result<(), Error> {
        write_config_with_indentation(
            writer,
            package_ecosystem,
            groups_by_workspace,
            ConfigIndentation::default(),
        )
    }

    fn sample_groups() -> DependencyGroups {
        [(
            String::new(),
            [
                (
                    "@acme".to_string(),
                    vec!["@acme/core".to_string(), "@acme/ui".to_string()],
                ),
                (
                    "react+react-dom".to_string(),
                    vec!["react".to_string(), "react-dom".to_string()],
                ),
            ]
            .into_iter()
            .collect(),
        )]
        .into_iter()
        .collect()
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

        write_config(&mut output, "npm", &sample_groups()).unwrap();

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
        let groups_by_workspace: DependencyGroups = [
            (
                "packages/client".to_string(),
                [(
                    "graphql".to_string(),
                    vec!["@apollo/client".to_string(), "graphql".to_string()],
                )]
                .into_iter()
                .collect(),
            ),
            (
                "packages/server".to_string(),
                [(
                    "graphql".to_string(),
                    vec!["@apollo/server".to_string(), "graphql".to_string()],
                )]
                .into_iter()
                .collect(),
            ),
        ]
        .into_iter()
        .collect();
        let mut output = Vec::new();

        write_config(&mut output, "npm", &groups_by_workspace).unwrap();

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
    fn serializes_the_requested_package_ecosystem() {
        let mut output = Vec::new();

        write_config(&mut output, "cargo", &sample_groups()).unwrap();

        let contents = String::from_utf8(output).unwrap();
        let config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&contents).unwrap();

        assert_eq!(config["updates"][0]["package-ecosystem"], "cargo");
    }

    #[test]
    fn writes_config_file_and_creates_parent_directory() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join(".github/dependabot.yml");

        write_config_file(&config_path, "npm", &sample_groups()).unwrap();

        assert!(config_path.is_file());

        let contents = std::fs::read_to_string(config_path).unwrap();
        assert!(contents.contains("updates:\n  - package-ecosystem: npm"));
        assert!(contents.contains("patterns:\n          - react"));

        let config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&contents).unwrap();
        let cooldown = &config["updates"][0]["cooldown"];

        assert_eq!(cooldown["semver-major-days"], 30);
        assert_eq!(cooldown["semver-minor-days"], 7);
        assert_eq!(cooldown["semver-patch-days"], 3);
    }

    #[test]
    fn preserves_existing_indentless_updates_sequence_when_merging() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("dependabot.yml");
        std::fs::write(
            &config_path,
            "version: 2\nupdates:\n- package-ecosystem: npm\n  directory: /\n  schedule:\n    interval: daily\n  groups:\n    legacy:\n      patterns:\n      - legacy-*\n",
        )
        .unwrap();

        write_config_file(&config_path, "npm", &sample_groups()).unwrap();

        let contents = std::fs::read_to_string(config_path).unwrap();
        assert!(contents.contains("updates:\n- package-ecosystem: npm"));
        assert!(
            contents.contains("patterns:\n      - '@acme/core'"),
            "{contents}"
        );
        let _: serde_yaml_ng::Value = serde_yaml_ng::from_str(&contents).unwrap();
    }

    #[test]
    fn infers_indentless_pattern_lists_when_existing_config_has_no_pattern_lists() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("dependabot.yml");
        std::fs::write(
            &config_path,
            "version: 2\nupdates:\n- package-ecosystem: npm\n  directory: /\n  schedule:\n    interval: daily\n",
        )
        .unwrap();

        write_config_file(&config_path, "npm", &sample_groups()).unwrap();

        let contents = std::fs::read_to_string(config_path).unwrap();
        assert!(contents.contains("patterns:\n      - '@acme/core'"));
        let _: serde_yaml_ng::Value = serde_yaml_ng::from_str(&contents).unwrap();
    }

    #[test]
    fn generates_config_from_an_empty_existing_file() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("dependabot.yml");
        std::fs::write(&config_path, "").unwrap();

        write_config_file(&config_path, "npm", &sample_groups()).unwrap();

        let contents = std::fs::read_to_string(config_path).unwrap();
        let config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&contents).unwrap();
        assert_eq!(config["version"], 2);
        assert_eq!(config["updates"][0]["package-ecosystem"], "npm");
        assert_eq!(
            config["updates"][0]["groups"]["acme"]["patterns"][0],
            "@acme/core"
        );
    }

    #[test]
    fn merges_generated_groups_into_existing_matching_updates() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("dependabot.yml");
        std::fs::write(
            &config_path,
            r#"version: 2
registries:
  private:
    type: npm-registry
    url: https://registry.example.com
updates:
  - package-ecosystem: npm
    directory: /
    schedule:
      interval: daily
    open-pull-requests-limit: 5
    groups:
      custom:
        patterns: ["custom-*"]
      acme:
        patterns: ["old-acme-pattern"]
"#,
        )
        .unwrap();

        write_config_file(&config_path, "npm", &sample_groups()).unwrap();

        let contents = std::fs::read_to_string(config_path).unwrap();
        let config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&contents).unwrap();
        let update = &config["updates"][0];

        assert_eq!(config["registries"]["private"]["type"], "npm-registry");
        assert_eq!(update["schedule"]["interval"], "daily");
        assert_eq!(update["open-pull-requests-limit"], 5);
        assert_eq!(update["groups"]["custom"]["patterns"][0], "custom-*");
        assert_eq!(
            update["groups"]["acme"]["patterns"],
            serde_yaml_ng::Value::Sequence(vec![
                serde_yaml_ng::Value::String("@acme/core".to_string()),
                serde_yaml_ng::Value::String("@acme/ui".to_string()),
            ])
        );
        assert_eq!(
            update["groups"]["react"]["patterns"],
            serde_yaml_ng::Value::Sequence(vec![
                serde_yaml_ng::Value::String("react".to_string()),
                serde_yaml_ng::Value::String("react-dom".to_string()),
            ])
        );
    }

    #[test]
    fn matches_existing_groups_by_dependencies_and_keeps_their_names() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("dependabot.yml");
        std::fs::write(
            &config_path,
            r#"version: 2
updates:
  - package-ecosystem: npm
    directory: /
    schedule:
      interval: daily
    groups:
      custom-acme-name:
        patterns:
          - "@acme/ui"
          - "@acme/core"
      untouched:
        patterns:
          - "custom-*"
"#,
        )
        .unwrap();

        write_config_file(&config_path, "npm", &sample_groups()).unwrap();

        let contents = std::fs::read_to_string(config_path).unwrap();
        let config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&contents).unwrap();
        let groups = &config["updates"][0]["groups"];

        assert!(groups.get("custom-acme-name").is_some());
        assert!(groups.get("acme").is_none());
        assert_eq!(
            groups["custom-acme-name"]["patterns"],
            serde_yaml_ng::Value::Sequence(vec![
                serde_yaml_ng::Value::String("@acme/ui".to_string()),
                serde_yaml_ng::Value::String("@acme/core".to_string()),
            ])
        );
        assert_eq!(groups["untouched"]["patterns"][0], "custom-*");
        assert_eq!(
            groups["react"]["patterns"],
            serde_yaml_ng::Value::Sequence(vec![
                serde_yaml_ng::Value::String("react".to_string()),
                serde_yaml_ng::Value::String("react-dom".to_string()),
            ])
        );
    }

    #[test]
    fn matches_existing_groups_by_partial_dependency_overlap() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("dependabot.yml");
        std::fs::write(
            &config_path,
            r#"version: 2
updates:
  - package-ecosystem: npm
    directory: /
    schedule:
      interval: daily
    groups:
      custom-acme-name:
        patterns:
          - "@acme/core"
          - "@acme/legacy"
          - "custom-only"
"#,
        )
        .unwrap();

        write_config_file(&config_path, "npm", &sample_groups()).unwrap();

        let contents = std::fs::read_to_string(config_path).unwrap();
        let config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&contents).unwrap();
        let groups = &config["updates"][0]["groups"];

        assert!(groups.get("custom-acme-name").is_some());
        assert!(groups.get("acme").is_none());
        assert_eq!(
            groups["custom-acme-name"]["patterns"],
            serde_yaml_ng::Value::Sequence(vec![
                serde_yaml_ng::Value::String("@acme/core".to_string()),
                serde_yaml_ng::Value::String("@acme/legacy".to_string()),
                serde_yaml_ng::Value::String("custom-only".to_string()),
                serde_yaml_ng::Value::String("@acme/ui".to_string()),
            ])
        );
    }

    #[test]
    fn merges_generated_dependencies_into_existing_wildcard_group() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("dependabot.yml");
        std::fs::write(
            &config_path,
            r#"version: 2
updates:
  - package-ecosystem: npm
    directory: /
    schedule:
      interval: daily
    groups:
      apollo-packages:
        patterns:
          - "@apollo/*"
"#,
        )
        .unwrap();
        let groups: DependencyGroups = [(
            String::new(),
            [(
                "@apollo".to_string(),
                vec![
                    "@apollo/client".to_string(),
                    "@apollo/server".to_string(),
                    "@apollo/utils".to_string(),
                ],
            )]
            .into_iter()
            .collect(),
        )]
        .into_iter()
        .collect();

        write_config_file(&config_path, "npm", &groups).unwrap();

        let contents = std::fs::read_to_string(config_path).unwrap();
        let config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&contents).unwrap();
        let groups = &config["updates"][0]["groups"];

        assert!(groups.get("apollo-packages").is_some());
        assert!(groups.get("apollo").is_none());
        assert_eq!(
            groups["apollo-packages"]["patterns"],
            serde_yaml_ng::Value::Sequence(vec![serde_yaml_ng::Value::String(
                "@apollo/*".to_string()
            )])
        );
    }

    #[test]
    fn does_not_match_groups_below_the_partial_overlap_threshold() {
        use yaml_edit::YamlFile;

        let left_file =
            YamlFile::from_str("patterns:\n  - alpha\n  - beta\n  - gamma\n  - delta\n").unwrap();
        let left =
            yaml_edit::YamlNode::Mapping(left_file.document().unwrap().as_mapping().unwrap());
        let right_file =
            YamlFile::from_str("patterns:\n  - alpha\n  - epsilon\n  - zeta\n  - eta\n").unwrap();
        let right =
            yaml_edit::YamlNode::Mapping(right_file.document().unwrap().as_mapping().unwrap());

        assert!(!groups_match_by_dependencies(&left, &right));
    }

    #[test]
    fn matches_dependency_patterns_with_wildcards() {
        assert!(dependency_pattern_matches(
            "@aws-sdk/*",
            "@aws-sdk/client-s3"
        ));
        assert!(dependency_pattern_matches(
            "@apollo/*/testing",
            "@apollo/client/testing"
        ));
        assert!(dependency_pattern_matches("*", "@apollo/client"));
        assert!(!dependency_pattern_matches("@apollo/*", "@apollo"));
        assert!(!dependency_pattern_matches(
            "@apollo/*",
            "@apollo-client/core"
        ));
    }

    #[test]
    fn preserves_existing_config_whitespace_and_comments() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("dependabot.yml");
        let original = r#"# manually maintained header
version:    2

registries:
  private:
    type: npm-registry # keep this comment
    url: https://registry.example.com

updates:
  - package-ecosystem: npm
    directory: /
    schedule:  { interval: daily } # keep schedule formatting
    open-pull-requests-limit:    5
    groups:
      custom:
        patterns: [ "custom-*" ] # preserve custom group formatting
      acme:
        patterns: ["old-acme-pattern"]
"#;
        std::fs::write(&config_path, original).unwrap();

        write_config_file(&config_path, "npm", &sample_groups()).unwrap();

        let contents = std::fs::read_to_string(config_path).unwrap();
        assert!(contents.contains("# manually maintained header\nversion:    2"));
        assert!(contents.contains("type: npm-registry # keep this comment"));
        assert!(contents.contains("schedule:  { interval: daily } # keep schedule formatting"));
        assert!(contents.contains("open-pull-requests-limit:    5"));
        assert!(contents.contains("patterns: [ \"custom-*\" ] # preserve custom group formatting"));

        let config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&contents).unwrap();
        assert_eq!(
            config["updates"][0]["groups"]["acme"]["patterns"][0],
            "@acme/core"
        );
    }

    #[test]
    fn does_not_overwrite_an_invalid_existing_config() {
        let temp_dir = tempfile::tempdir().unwrap();
        let config_path = temp_dir.path().join("dependabot.yml");
        let original = "not: [valid";
        std::fs::write(&config_path, original).unwrap();

        assert!(write_config_file(&config_path, "npm", &sample_groups()).is_err());
        assert_eq!(std::fs::read_to_string(config_path).unwrap(), original);
    }

    #[test]
    fn serializes_precombined_workspace_groups_as_one_root_update() {
        let groups_by_workspace: DependencyGroups = [(
            String::new(),
            [
                (
                    "graphql".to_string(),
                    vec!["@apollo/client".to_string(), "@apollo/server".to_string()],
                ),
                (
                    "shared-dependencies".to_string(),
                    vec!["graphql".to_string()],
                ),
                ("react".to_string(), vec!["react".to_string()]),
            ]
            .into_iter()
            .collect(),
        )]
        .into_iter()
        .collect();
        let mut output = Vec::new();

        write_config(&mut output, "npm", &groups_by_workspace).unwrap();

        let contents = String::from_utf8(output).unwrap();
        let config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&contents).unwrap();
        let updates = config["updates"].as_sequence().unwrap();

        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0]["directory"], "/");
        assert_eq!(
            updates[0]["groups"]["graphql"]["patterns"],
            serde_yaml_ng::Value::Sequence(vec![
                serde_yaml_ng::Value::String("@apollo/client".to_string()),
                serde_yaml_ng::Value::String("@apollo/server".to_string()),
            ])
        );
        assert_eq!(
            updates[0]["groups"]["shared-dependencies"]["patterns"],
            serde_yaml_ng::Value::Sequence(vec![serde_yaml_ng::Value::String(
                "graphql".to_string()
            )])
        );
        assert_eq!(
            updates[0]["groups"]["react"]["patterns"],
            serde_yaml_ng::Value::Sequence(vec![serde_yaml_ng::Value::String("react".to_string())])
        );
    }

    #[test]
    fn combines_apollo_monorepo_without_duplicate_dependency_patterns() {
        let example_root =
            Path::new(env!("CARGO_MANIFEST_DIR")).join("examples/npm/apollo-monorepo");
        let crate::ecosystems::Detection::Detected(project) =
            crate::ecosystems::detect(&example_root, true, true).unwrap()
        else {
            panic!("expected project to be detected");
        };
        let debug_report = project.debug_report.as_deref().unwrap();
        let mut output = Vec::new();

        write_config(
            &mut output,
            project.package_ecosystem,
            &project.groups_by_workspace,
        )
        .unwrap();

        let contents = String::from_utf8(output).unwrap();
        let config: serde_yaml_ng::Value = serde_yaml_ng::from_str(&contents).unwrap();
        let updates = config["updates"].as_sequence().unwrap();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0]["directory"], "/");

        let patterns = updates[0]["groups"]
            .as_mapping()
            .unwrap()
            .values()
            .flat_map(|group| group["patterns"].as_sequence().unwrap())
            .collect::<Vec<_>>();
        let unique_patterns = patterns.iter().collect::<std::collections::HashSet<_>>();

        assert_eq!(
            updates[0]["groups"]["shared-dependencies"]["patterns"],
            serde_yaml_ng::Value::Sequence(vec![serde_yaml_ng::Value::String(
                "graphql".to_string()
            )])
        );
        assert!(debug_report.contains("Group: shared-dependencies"));
        assert!(debug_report.contains("└── graphql"));
        for (name, group) in updates[0]["groups"].as_mapping().unwrap() {
            if name != "shared-dependencies" {
                assert!(
                    group["patterns"]
                        .as_sequence()
                        .unwrap()
                        .iter()
                        .all(|pattern| pattern != "graphql")
                );
            }
        }
        assert_eq!(unique_patterns.len(), patterns.len());
    }
}
