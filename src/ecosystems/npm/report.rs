use std::{collections::BTreeMap, fmt::Write as _};

use super::NpmDependency;
use crate::ecosystems::{DependencyGroups, WorkspaceGroups};

pub(super) fn render_combined_groups_report(groups_by_workspace: &DependencyGroups) -> String {
    let groups = groups_by_workspace
        .get("")
        .expect("combined workspace groups must be stored at the repository root");
    let mut report = String::from("Combined workspace groups:\n");
    for (group_index, (group, patterns)) in groups.iter().enumerate() {
        let is_last_group = group_index + 1 == groups.len();
        let group_branch = if is_last_group {
            "└── "
        } else {
            "├── "
        };
        let pattern_indent = if is_last_group { "    " } else { "│   " };
        let _ = writeln!(report, "{group_branch}Group: {group}");
        for (pattern_index, pattern) in patterns.iter().enumerate() {
            let pattern_branch = if pattern_index + 1 == patterns.len() {
                "└── "
            } else {
                "├── "
            };
            let _ = writeln!(report, "{pattern_indent}{pattern_branch}{pattern}");
        }
    }
    report
}

pub(super) fn render_dependency_report(
    dependencies_by_workspace: &BTreeMap<String, Vec<NpmDependency>>,
    groups_by_workspace: &DependencyGroups,
) -> String {
    let mut report = String::new();

    for (workspace, dependencies) in dependencies_by_workspace {
        render_workspace_report(
            &mut report,
            workspace,
            dependencies,
            groups_by_workspace.get(workspace),
        );
    }

    report
}

fn render_workspace_report(
    report: &mut String,
    workspace: &str,
    dependencies: &[NpmDependency],
    groups: Option<&WorkspaceGroups>,
) {
    let directory = if workspace.is_empty() { "/" } else { workspace };
    let _ = writeln!(report, "Workspace: {directory}");
    let ungrouped = ungrouped_dependencies(dependencies, groups);
    let group_count = groups.map_or(0, WorkspaceGroups::len);

    for (group_index, (group_name, members)) in groups
        .into_iter()
        .flat_map(WorkspaceGroups::iter)
        .enumerate()
    {
        let is_last_section = group_index + 1 == group_count && ungrouped.is_empty();
        let child_prefix =
            append_section_heading(report, &format!("Group: {group_name}"), is_last_section);
        append_group_tree(report, dependencies, group_name, members, &child_prefix);
    }

    if !ungrouped.is_empty() {
        let child_prefix = append_section_heading(report, "Ungrouped", true);
        append_ungrouped_dependencies(report, &ungrouped, &child_prefix);
    }
}

fn ungrouped_dependencies(
    dependencies: &[NpmDependency],
    groups: Option<&WorkspaceGroups>,
) -> Vec<String> {
    let assigned = groups
        .into_iter()
        .flat_map(WorkspaceGroups::iter)
        .flat_map(|(_, members)| members.iter().map(String::as_str))
        .collect::<std::collections::HashSet<_>>();
    let mut ungrouped = dependencies
        .iter()
        .filter(|dependency| !assigned.contains(dependency.name.as_str()))
        .map(|dependency| dependency.name.clone())
        .collect::<Vec<_>>();
    ungrouped.sort();
    ungrouped
}

fn append_section_heading(report: &mut String, label: &str, is_last: bool) -> String {
    let (branch, child_prefix) = if is_last {
        ("└── ", "    ")
    } else {
        ("├── ", "│   ")
    };
    let _ = writeln!(report, "{branch}{label}");
    child_prefix.to_string()
}

fn append_group_tree(
    report: &mut String,
    dependencies: &[NpmDependency],
    group: &str,
    members: &[String],
    child_prefix: &str,
) {
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

    let mut tree_report = DependencyTreeReport {
        output: report,
        dependencies,
        members,
        group,
        visited: std::collections::HashSet::new(),
    };
    for (member_index, member) in roots.iter().enumerate() {
        tree_report.append(member, child_prefix, member_index + 1 == roots.len(), false);
    }

    let mut remaining = members
        .iter()
        .filter(|member| !tree_report.visited.contains(member.as_str()))
        .collect::<Vec<_>>();
    remaining.sort();
    for member in remaining {
        let is_last_member = tree_report.visited.len() + 1 == members.len();
        tree_report.append(member, child_prefix, is_last_member, false);
    }
}

fn append_ungrouped_dependencies(report: &mut String, dependencies: &[String], indent: &str) {
    for (member_index, member) in dependencies.iter().enumerate() {
        let branch = if member_index + 1 == dependencies.len() {
            "└── "
        } else {
            "├── "
        };
        let _ = writeln!(report, "{indent}{branch}{member}");
    }
}

struct DependencyTreeReport<'a> {
    output: &'a mut String,
    dependencies: &'a [NpmDependency],
    members: &'a [String],
    group: &'a str,
    visited: std::collections::HashSet<String>,
}

impl DependencyTreeReport<'_> {
    fn append(&mut self, name: &str, indent: &str, is_last: bool, is_peer_link: bool) {
        let branch = if is_last { "└── " } else { "├── " };
        let _ = write!(self.output, "{indent}{branch}{name}");
        if is_peer_link {
            let _ = write!(self.output, " [peer]");
        }
        if !self.visited.insert(name.to_string()) {
            self.output.push('\n');
            return;
        }

        let dependency = self
            .dependencies
            .iter()
            .find(|dependency| dependency.name == name);
        if !is_peer_link
            && let Some(dependency) = dependency
            && let Some(reason) = dependency_group_reason(dependency, self.group, self.members)
        {
            let _ = write!(self.output, "{reason}");
        }
        self.output.push('\n');

        let mut peers = {
            let Some(dependency) = dependency else {
                return;
            };
            let member_names = self
                .members
                .iter()
                .map(String::as_str)
                .collect::<std::collections::HashSet<_>>();
            dependency
                .peer_dependencies
                .iter()
                .filter(|peer| member_names.contains(peer.as_str()))
                .cloned()
                .collect::<Vec<_>>()
        };
        peers.sort();

        let child_indent = format!("{indent}{}", if is_last { "    " } else { "│   " });
        for (peer_index, peer) in peers.iter().enumerate() {
            self.append(peer, &child_indent, peer_index + 1 == peers.len(), true);
        }
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

#[cfg(test)]
mod tests {
    use std::{collections::BTreeMap, path::Path};

    use super::*;

    #[test]
    fn renders_groups_with_peer_dependency_links() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/npm-peer");
        let dependencies = super::super::read_npm_dependency_metadata(&root).unwrap();
        let groups = dependencies
            .iter()
            .map(|(workspace, dependencies)| {
                (
                    workspace.clone(),
                    super::super::group_npm_dependencies(dependencies),
                )
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
                (
                    workspace.clone(),
                    super::super::group_npm_dependencies(dependencies),
                )
            })
            .collect();

        let report = render_dependency_report(&dependencies, &groups);

        assert!(report.contains("@acme/core (same npm scope @acme)"));
        assert!(report.contains("@types/express [types]"));
    }
}
