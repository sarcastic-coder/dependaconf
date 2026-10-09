use std::collections::BTreeMap;

#[derive(serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) struct DependabotUpdate {
    pub(super) package_ecosystem: String,
    pub(super) directory: String,
    pub(super) schedule: DependabotSchedule,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) groups: Option<BTreeMap<String, DependabotGroup>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(super) cooldown: Option<DependabotCooldown>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "kebab-case")]
pub(super) struct DependabotCooldown {
    pub(super) semver_major_days: u8,
    pub(super) semver_minor_days: u8,
    pub(super) semver_patch_days: u8,
}

#[derive(serde::Serialize)]
pub(super) struct DependabotSchedule {
    pub(super) interval: String,
}

#[derive(serde::Serialize)]
pub(super) struct DependabotGroup {
    pub(super) patterns: Vec<String>,
}
