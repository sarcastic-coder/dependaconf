use yaml_edit::YamlNode;

use super::config::Error;

pub(super) fn merge_generated_groups_losslessly(
    existing: &yaml_edit::YamlFile,
    generated: &yaml_edit::YamlFile,
) -> Result<(), Error> {
    let invalid_config = || {
        Error::InvalidConfig(
            "existing Dependabot config must be a mapping with an updates sequence",
        )
    };
    let existing = existing
        .document()
        .and_then(|document| document.as_mapping())
        .ok_or_else(invalid_config)?;
    let generated = generated
        .document()
        .and_then(|document| document.as_mapping())
        .ok_or_else(invalid_config)?;
    let generated_updates = generated
        .get_sequence("updates")
        .ok_or_else(invalid_config)?;

    let Some(updates_node) = existing.get("updates") else {
        existing.set("updates", generated_updates);
        return Ok(());
    };
    let updates = updates_node.as_sequence().ok_or_else(invalid_config)?;

    for generated_update in generated_updates.values() {
        let Some(generated_update) = generated_update.as_mapping() else {
            continue;
        };
        let Some(ecosystem) = generated_update.get("package-ecosystem") else {
            continue;
        };
        let Some(directory) = generated_update.get("directory") else {
            continue;
        };
        let matching_update = (0..updates.len()).find_map(|index| {
            let update = updates.get(index)?;
            let update = update.as_mapping()?;
            let matches = update
                .get("package-ecosystem")
                .is_some_and(|value| value.yaml_eq(&ecosystem))
                && update
                    .get("directory")
                    .is_some_and(|value| value.yaml_eq(&directory));
            matches.then_some(update.clone())
        });

        let Some(matching_update) = matching_update else {
            updates.push(generated_update);
            continue;
        };
        let Some(generated_groups) = generated_update.get_mapping("groups") else {
            continue;
        };
        let Some(groups_node) = matching_update.get("groups") else {
            matching_update.set("groups", generated_groups);
            continue;
        };
        let Some(groups) = groups_node.as_mapping() else {
            return Err(Error::InvalidConfig(
                "groups in existing Dependabot update must be a mapping",
            ));
        };

        for (name, group) in generated_groups.iter() {
            let YamlNode::Scalar(generated_name) = name else {
                continue;
            };
            let generated_name = generated_name.as_string();
            let existing_name = groups.iter().find_map(|(existing_name, existing_group)| {
                let YamlNode::Scalar(existing_name) = existing_name else {
                    return None;
                };
                groups_match_by_dependencies(&existing_group, &group)
                    .then(|| (existing_name.as_string(), existing_group))
            });
            if let Some((existing_name, existing_group)) = existing_name {
                let patterns_unchanged = group_patterns(&existing_group) == group_patterns(&group);
                if existing_name != generated_name || !patterns_unchanged {
                    merge_group_patterns(&existing_group, &group);
                    continue;
                }
            }
            groups.set(generated_name, group);
        }
    }

    Ok(())
}

fn group_patterns(group: &yaml_edit::YamlNode) -> Option<Vec<String>> {
    let group = group.as_mapping()?;
    let patterns = group.get_sequence("patterns")?;
    let mut patterns = patterns
        .values()
        .map(|pattern| pattern.as_scalar().map(|scalar| scalar.as_string()))
        .collect::<Option<Vec<_>>>()?;
    patterns.sort();
    patterns.dedup();
    Some(patterns)
}

pub(super) fn groups_match_by_dependencies(
    left: &yaml_edit::YamlNode,
    right: &yaml_edit::YamlNode,
) -> bool {
    let (Some(left), Some(right)) = (group_patterns(left), group_patterns(right)) else {
        return false;
    };
    let overlap = right
        .iter()
        .filter(|dependency| {
            left.iter()
                .any(|pattern| dependency_pattern_matches(pattern, dependency))
        })
        .count();
    overlap.saturating_mul(2) >= left.len().min(right.len()) && overlap > 0
}

pub(super) fn dependency_pattern_matches(pattern: &str, dependency: &str) -> bool {
    let pattern: Vec<_> = pattern.chars().collect();
    let dependency: Vec<_> = dependency.chars().collect();
    let (mut pattern_index, mut dependency_index) = (0, 0);
    let mut star_index = None;
    let mut retry_dependency_index = 0;

    while dependency_index < dependency.len() {
        if pattern.get(pattern_index) == Some(&dependency[dependency_index]) {
            pattern_index += 1;
            dependency_index += 1;
        } else if pattern.get(pattern_index) == Some(&'*') {
            star_index = Some(pattern_index);
            pattern_index += 1;
            retry_dependency_index = dependency_index;
        } else if let Some(star_index) = star_index {
            pattern_index = star_index + 1;
            retry_dependency_index += 1;
            dependency_index = retry_dependency_index;
        } else {
            return false;
        }
    }

    pattern[pattern_index..]
        .iter()
        .all(|character| *character == '*')
}

fn merge_group_patterns(existing: &yaml_edit::YamlNode, generated: &yaml_edit::YamlNode) {
    let (Some(existing_patterns), Some(generated_patterns)) = (
        existing
            .as_mapping()
            .and_then(|group| group.get_sequence("patterns")),
        group_patterns(generated),
    ) else {
        return;
    };
    let mut existing_values = existing_patterns
        .values()
        .filter_map(|pattern| pattern.as_scalar().map(|scalar| scalar.as_string()))
        .collect::<std::collections::HashSet<_>>();

    for pattern in generated_patterns {
        let is_covered = existing_values
            .iter()
            .any(|existing| dependency_pattern_matches(existing, &pattern));
        if !is_covered && existing_values.insert(pattern.clone()) {
            existing_patterns.push(pattern);
        }
    }
}
