#[derive(Clone, Copy)]
pub(super) struct ConfigIndentation {
    pub(super) update_item: usize,
    pub(super) pattern_item_offset: usize,
}

impl Default for ConfigIndentation {
    fn default() -> Self {
        Self {
            update_item: 2,
            pattern_item_offset: 2,
        }
    }
}

pub(super) fn yaml_scalar(value: &str) -> String {
    serde_yaml_ng::to_string(&value.to_string())
        .unwrap_or_else(|_| format!("\"{value}\""))
        .trim_end()
        .to_string()
}

pub(super) fn detect_config_indentation(contents: &str) -> ConfigIndentation {
    let mut indentation = ConfigIndentation::default();
    let lines: Vec<_> = contents.lines().collect();
    let mut found_pattern_list = false;

    for (index, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if trimmed == "updates:" {
            if let Some(item) = lines[index + 1..]
                .iter()
                .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
                .filter(|line| line.trim_start().starts_with("- "))
            {
                indentation.update_item = item.len() - item.trim_start().len();
            }
        }

        if trimmed == "patterns:" {
            let key_indent = line.len() - line.trim_start().len();
            if let Some(item) = lines[index + 1..]
                .iter()
                .find(|line| !line.trim().is_empty() && !line.trim_start().starts_with('#'))
                .filter(|line| line.trim_start().starts_with("- "))
            {
                let item_indent = item.len() - item.trim_start().len();
                indentation.pattern_item_offset = item_indent.saturating_sub(key_indent);
                found_pattern_list = true;
                break;
            }
        }
    }

    if !found_pattern_list && indentation.update_item == 0 {
        indentation.pattern_item_offset = 0;
    }

    indentation
}

pub(super) fn dependabot_group_identifier(group: &str) -> String {
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
