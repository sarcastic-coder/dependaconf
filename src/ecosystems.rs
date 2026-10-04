use std::{
    collections::{BTreeMap, HashMap},
    path::Path,
};

mod npm;

pub(super) type DependencyGroups = BTreeMap<String, HashMap<String, Vec<String>>>;

pub(super) struct ProjectDependencies {
    pub(super) package_ecosystem: &'static str,
    pub(super) groups_by_workspace: DependencyGroups,
    pub(super) debug_report: Option<String>,
}

pub(super) fn detect(
    root: &Path,
    include_debug_report: bool,
) -> Result<Option<ProjectDependencies>, Box<dyn std::error::Error>> {
    if !npm::is_project(root) {
        return Ok(None);
    }

    let (groups_by_workspace, debug_report) = npm::dependency_analysis(root, include_debug_report)?;

    Ok(Some(ProjectDependencies {
        package_ecosystem: npm::DEPENDABOT_NAME,
        groups_by_workspace,
        debug_report,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detects_project_and_returns_ecosystem_neutral_groups() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/npm-peer");

        let project = detect(&root, false).unwrap().unwrap();

        assert_eq!(project.package_ecosystem, "npm");
        assert!(project.groups_by_workspace.contains_key(""));
    }

    #[test]
    fn returns_none_when_no_supported_ecosystem_is_detected() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/empty");

        assert!(detect(&root, false).unwrap().is_none());
    }
}
