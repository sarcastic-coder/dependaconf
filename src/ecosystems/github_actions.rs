use std::path::Path;

use crate::ecosystems::DependencyGroups;

pub(super) const DEPENDABOT_NAME: &str = "github-actions";

pub(super) fn is_project(root: &Path) -> bool {
    root.join(".github/workflows").is_dir()
}

pub(super) fn dependency_analysis(_root: &Path) -> Result<DependencyGroups, super::Error> {
    Ok(DependencyGroups::default())
}
