use std::{
    collections::{BTreeMap, HashMap},
    fs::{read_dir, read_to_string},
    path::{Component, Path},
};

use super::{DependencyMetadata, Error};

#[derive(serde::Deserialize, Default)]
struct PackageJson {
    name: Option<String>,
    #[serde(default)]
    dependencies: HashMap<String, String>,
    #[serde(rename = "devDependencies", default)]
    dev_dependencies: HashMap<String, String>,
    #[serde(rename = "peerDependencies", default)]
    peer_dependencies: HashMap<String, String>,
    workspaces: Option<serde_json::Value>,
}

pub(super) fn read_dependency_metadata(
    root: &Path,
) -> Result<BTreeMap<String, Vec<DependencyMetadata>>, Error> {
    let lock_path = root.join("yarn.lock");
    if !lock_path.is_file() {
        return Err(Error::MissingLockfile);
    }

    let contents = read_to_string(lock_path)?;
    let lock_entries = parse_lockfile(&contents)?;
    let root_package = read_package_json(&root.join("package.json"))?;
    let workspace_patterns = workspace_patterns(&root_package);
    let mut manifests = vec![(String::new(), root_package)];

    if !workspace_patterns.is_empty() {
        collect_workspace_manifests(root, root, &workspace_patterns, &mut manifests)?;
    }

    let local_workspaces: HashMap<_, _> = manifests
        .iter()
        .filter_map(|(_, package)| {
            package
                .name
                .as_ref()
                .map(|name| (name.as_str(), package.peer_dependencies.keys()))
        })
        .map(|(name, peers)| (name.to_string(), peers.cloned().collect::<Vec<_>>()))
        .collect();

    let mut dependencies_by_workspace = BTreeMap::new();
    for (workspace, manifest) in manifests {
        let dependencies = manifest
            .dependencies
            .iter()
            .chain(manifest.dev_dependencies.iter())
            .map(|(name, range)| {
                let peer_dependencies = if range.starts_with("workspace:")
                    && let Some(peers) = local_workspaces.get(name)
                {
                    peers.clone()
                } else {
                    find_peer_dependencies(&lock_entries, name, range)
                        .or_else(|| local_workspaces.get(name).cloned())
                        .ok_or_else(|| Error::MissingDirectDependency(name.clone()))?
                };

                Ok(DependencyMetadata {
                    name: name.clone(),
                    peer_dependencies,
                })
            })
            .collect::<Result<Vec<_>, Error>>()?;
        dependencies_by_workspace.insert(workspace, dependencies);
    }

    Ok(dependencies_by_workspace)
}

fn read_package_json(path: &Path) -> Result<PackageJson, Error> {
    let contents = read_to_string(path)?;
    serde_json::from_str(&contents).map_err(Error::PackageJson)
}

fn workspace_patterns(package: &PackageJson) -> Vec<String> {
    let Some(workspaces) = &package.workspaces else {
        return Vec::new();
    };
    let patterns = workspaces.as_array().or_else(|| {
        workspaces
            .get("packages")
            .and_then(serde_json::Value::as_array)
    });

    patterns
        .into_iter()
        .flatten()
        .filter_map(serde_json::Value::as_str)
        .map(str::to_string)
        .collect()
}

fn collect_workspace_manifests(
    root: &Path,
    directory: &Path,
    patterns: &[String],
    manifests: &mut Vec<(String, PackageJson)>,
) -> Result<(), Error> {
    for entry in read_dir(directory)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            if matches!(
                entry.file_name().to_str(),
                Some("node_modules" | ".git" | ".yarn")
            ) {
                continue;
            }
            collect_workspace_manifests(root, &path, patterns, manifests)?;
        } else if file_type.is_file() && entry.file_name() == "package.json" {
            let relative_path = path
                .parent()
                .and_then(|parent| parent.strip_prefix(root).ok())
                .map(path_to_slashes)
                .unwrap_or_default();
            if is_workspace_path(&relative_path, patterns) {
                manifests.push((relative_path, read_package_json(&path)?));
            }
        }
    }

    Ok(())
}

fn path_to_slashes(path: &Path) -> String {
    path.components()
        .filter_map(|component| match component {
            Component::Normal(value) => value.to_str(),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("/")
}

fn is_workspace_path(path: &str, patterns: &[String]) -> bool {
    let mut matched = false;
    for pattern in patterns {
        let excluded = pattern.starts_with('!');
        let pattern = pattern.trim_start_matches('!').trim_start_matches("./");
        if pattern_matches(pattern, path) {
            matched = !excluded;
        }
    }
    matched
}

fn pattern_matches(pattern: &str, path: &str) -> bool {
    fn match_segments(pattern: &[&str], path: &[&str]) -> bool {
        match pattern.split_first() {
            None => path.is_empty(),
            Some((&"**", remaining)) => {
                match_segments(remaining, path)
                    || (!path.is_empty() && match_segments(pattern, &path[1..]))
            }
            Some((segment, remaining)) => {
                !path.is_empty()
                    && segment_matches(segment, path[0])
                    && match_segments(remaining, &path[1..])
            }
        }
    }

    fn segment_matches(pattern: &str, value: &str) -> bool {
        match pattern.split_once('*') {
            None => pattern == value,
            Some((prefix, remaining)) if value.starts_with(prefix) => {
                let value = &value[prefix.len()..];
                (0..=value.len())
                    .filter(|index| value.is_char_boundary(*index))
                    .any(|index| segment_matches(remaining, &value[index..]))
            }
            Some(_) => false,
        }
    }

    match_segments(
        &pattern.split('/').collect::<Vec<_>>(),
        &path.split('/').collect::<Vec<_>>(),
    )
}

fn parse_lockfile(contents: &str) -> Result<HashMap<String, Vec<String>>, Error> {
    if contents.lines().any(|line| line.trim() == "__metadata:") {
        parse_modern_lockfile(contents)
    } else {
        parse_classic_lockfile(contents)
    }
}

fn parse_modern_lockfile(contents: &str) -> Result<HashMap<String, Vec<String>>, Error> {
    let lockfile: serde_yaml_ng::Value = serde_yaml_ng::from_str(contents)?;
    let Some(entries) = lockfile.as_mapping() else {
        return Err(Error::InvalidYarnLock(
            "expected a mapping of package selectors".to_string(),
        ));
    };

    let mut parsed = HashMap::new();
    for (key, value) in entries {
        let Some(key) = key.as_str() else {
            continue;
        };
        if key == "__metadata" {
            continue;
        }
        let peers = value
            .get("peerDependencies")
            .and_then(serde_yaml_ng::Value::as_mapping)
            .into_iter()
            .flat_map(|dependencies| dependencies.keys())
            .filter_map(serde_yaml_ng::Value::as_str)
            .map(str::to_string)
            .collect::<Vec<_>>();
        for selector in split_selectors(key) {
            parsed.insert(selector, peers.clone());
        }
    }

    if parsed.is_empty() {
        return Err(Error::InvalidYarnLock(
            "no package entries were found".to_string(),
        ));
    }
    Ok(parsed)
}

fn parse_classic_lockfile(contents: &str) -> Result<HashMap<String, Vec<String>>, Error> {
    let mut parsed = HashMap::new();
    let mut selectors = Vec::new();
    let mut peer_dependencies = Vec::new();
    let mut in_peer_dependencies = false;
    let mut found_entry = false;

    let save_entry = |parsed: &mut HashMap<String, Vec<String>>,
                      selectors: &mut Vec<String>,
                      peer_dependencies: &mut Vec<String>| {
        for selector in selectors.drain(..) {
            parsed.insert(selector, peer_dependencies.clone());
        }
        peer_dependencies.clear();
    };

    for line in contents.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if !line
            .chars()
            .next()
            .is_some_and(|character| character.is_whitespace())
            && trimmed.ends_with(':')
        {
            save_entry(&mut parsed, &mut selectors, &mut peer_dependencies);
            selectors = split_selectors(trimmed.trim_end_matches(':'));
            in_peer_dependencies = false;
            found_entry = true;
        } else if line.starts_with("  ") && !line.starts_with("    ") {
            in_peer_dependencies = trimmed == "peerDependencies:";
        } else if in_peer_dependencies
            && line.starts_with("    ")
            && let Some((name, _)) = trimmed.split_once(char::is_whitespace)
        {
            peer_dependencies.push(unquote(name).to_string());
        }
    }
    save_entry(&mut parsed, &mut selectors, &mut peer_dependencies);

    if !found_entry {
        return Err(Error::InvalidYarnLock(
            "no package entries were found".to_string(),
        ));
    }
    Ok(parsed)
}

fn split_selectors(value: &str) -> Vec<String> {
    let mut selectors = Vec::new();
    let mut start = 0;
    let mut quote = None;
    for (index, character) in value.char_indices() {
        match character {
            '"' | '\'' if quote == Some(character) => quote = None,
            '"' | '\'' if quote.is_none() => quote = Some(character),
            ',' if quote.is_none() => {
                selectors.push(unquote(value[start..index].trim()).to_string());
                start = index + 1;
            }
            _ => {}
        }
    }
    let remaining = value[start..].trim();
    if !remaining.is_empty() {
        selectors.push(unquote(remaining).to_string());
    }
    selectors
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            value
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(value)
}

fn find_peer_dependencies(
    entries: &HashMap<String, Vec<String>>,
    name: &str,
    range: &str,
) -> Option<Vec<String>> {
    let expected_selector = format!("{name}@{range}");
    let expected_npm_selector = format!("{name}@npm:{range}");
    entries.iter().find_map(|(selector, peers)| {
        (selector == &expected_selector || selector == &expected_npm_selector)
            .then(|| peers.clone())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_classic_yarn_lock_peer_dependencies() {
        let lockfile = r#"
# yarn lockfile v1
"@scope/plugin@^1.0.0", "@scope/plugin@^1.1.0":
  version "1.2.3"
  peerDependencies:
    react "^18.0.0"
"#;

        let entries = parse_lockfile(lockfile).unwrap();

        assert_eq!(
            entries.get("@scope/plugin@^1.0.0"),
            Some(&vec!["react".to_string()])
        );
        assert_eq!(
            entries.get("@scope/plugin@^1.1.0"),
            Some(&vec!["react".to_string()])
        );
    }

    #[test]
    fn parses_modern_yarn_lock_peer_dependencies() {
        let lockfile = r#"
__metadata:
  version: 6
  cacheKey: 10c0

"@scope/plugin@npm:^1.0.0":
  version: 1.2.3
  peerDependencies:
    react: ^18.0.0
"#;

        let entries = parse_lockfile(lockfile).unwrap();

        assert_eq!(
            entries.get("@scope/plugin@npm:^1.0.0"),
            Some(&vec!["react".to_string()])
        );
    }

    #[test]
    fn handles_workspace_globs() {
        assert!(pattern_matches("packages/*", "packages/api"));
        assert!(pattern_matches("packages/**", "packages/tools/eslint"));
        assert!(!pattern_matches("packages/*", "apps/api"));
    }

    #[test]
    fn reads_classic_yarn_dependencies_from_package_json() {
        let temp_dir = tempfile::tempdir().unwrap();
        std::fs::write(
            temp_dir.path().join("package.json"),
            r#"{"dependencies":{"react":"^18.0.0","react-dom":"^18.0.0"}}"#,
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("yarn.lock"),
            r#"
# yarn lockfile v1
react-dom@^18.0.0:
  version "18.2.0"
react@^18.0.0:
  version "18.2.0"
"#,
        )
        .unwrap();

        let dependencies = read_dependency_metadata(temp_dir.path()).unwrap();
        assert_eq!(
            dependencies[""]
                .iter()
                .map(|dependency| dependency.name.as_str())
                .collect::<std::collections::HashSet<_>>(),
            std::collections::HashSet::from(["react", "react-dom"])
        );
    }

    #[test]
    fn reads_modern_yarn_peer_metadata_and_workspaces() {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace = temp_dir.path().join("packages/app");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(
            temp_dir.path().join("package.json"),
            r#"{"workspaces":["packages/*"],"dependencies":{"plugin":"^1.0.0"}}"#,
        )
        .unwrap();
        std::fs::write(
            workspace.join("package.json"),
            r#"{"name":"app","dependencies":{"react":"^18.0.0"}}"#,
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("yarn.lock"),
            r#"
__metadata:
  version: 6
  cacheKey: 10c0

plugin@npm:^1.0.0:
  version: 1.2.3
  peerDependencies:
    react: ^18.0.0

react@npm:^18.0.0:
  version: 18.2.0
"#,
        )
        .unwrap();

        let dependencies = read_dependency_metadata(temp_dir.path()).unwrap();

        assert_eq!(
            dependencies[""][0].peer_dependencies,
            vec!["react".to_string()]
        );
        assert_eq!(dependencies["packages/app"][0].name, "react");
    }

    #[test]
    fn uses_local_workspace_peer_metadata_without_a_lock_entry() {
        let temp_dir = tempfile::tempdir().unwrap();
        let workspace = temp_dir.path().join("packages/plugin");
        std::fs::create_dir_all(&workspace).unwrap();
        std::fs::write(
            temp_dir.path().join("package.json"),
            r#"{"workspaces":["packages/*"],"dependencies":{"plugin":"^1.0.0"}}"#,
        )
        .unwrap();
        std::fs::write(
            workspace.join("package.json"),
            r#"{"name":"plugin","peerDependencies":{"react":"^18.0.0"}}"#,
        )
        .unwrap();
        std::fs::write(
            temp_dir.path().join("yarn.lock"),
            r#"
# yarn lockfile v1
"react@^18.0.0":
  version "18.2.0"
"#,
        )
        .unwrap();

        let dependencies = read_dependency_metadata(temp_dir.path()).unwrap();

        assert_eq!(
            dependencies[""][0].peer_dependencies,
            vec!["react".to_string()]
        );
    }
}
