// Audit-specific project TOML types.
// Exports: ProjectAuditConfig plus the internal parsed project file wrapper.
// Deps: serde and parent `ProjectConfig`.

use serde::Deserialize;

use super::ProjectConfig;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct ProjectFile {
    #[serde(rename = "project")]
    pub project: ProjectConfig,
    #[serde(default)]
    pub audit: ProjectAuditConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectAuditConfig {
    #[serde(default)]
    pub auto: bool,
}
