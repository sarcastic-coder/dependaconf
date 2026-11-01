use std::path::Path;

pub(super) const DEPENDABOT_NAME: &str = "github-actions";

pub(super) fn is_project(root: &Path) -> bool {
    root.join(".github/workflows").is_dir()
}
