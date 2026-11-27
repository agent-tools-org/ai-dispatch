// Audit-specific project TOML types and the project stage that sets the audit bar.
// Exports: ProjectAuditConfig, ProjectStage, plus the internal parsed project file wrapper.
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
    pub backup: Option<crate::workspace::backup::BackupProjectConfig>,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default, deny_unknown_fields)]
pub struct ProjectAuditConfig {
    #[serde(default)]
    pub auto: bool,
}

/// `[project] stage`: how mature the project is, which sets how strict audit reports are.
/// Unset audits exactly like `stable`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ProjectStage {
    /// Idea or draft: speed of exploration over completeness; bugs are expected.
    Prototype,
    /// Development or experiment: the design still moves; bugs off the main path are tolerated.
    Experimental,
    /// Production or maintenance: users depend on it; the full audit bar applies.
    Stable,
}

impl ProjectStage {
    /// Every stage, for tests that check generated facts against the guide.
    #[cfg(test)]
    pub const ALL: [Self; 3] = [Self::Prototype, Self::Experimental, Self::Stable];

    pub fn label(self) -> &'static str {
        match self {
            Self::Prototype => "prototype",
            Self::Experimental => "experimental",
            Self::Stable => "stable",
        }
    }

    /// One line for dispatchers and `aid project show`.
    pub fn summary(self) -> &'static str {
        match self {
            Self::Prototype => "audit bar lowered: FIX/BLOCK only for data loss, a security break, or an unusable change",
            Self::Experimental => "audit bar lowered: FIX/BLOCK only for data loss, a security break, or a broken main path",
            Self::Stable => "full audit bar",
        }
    }

    /// Narrows the audit report's high rating; `None` keeps the full bar.
    pub fn audit_calibration(self) -> Option<&'static str> {
        match self {
            Self::Prototype => Some(PROTOTYPE_AUDIT),
            Self::Experimental => Some(EXPERIMENTAL_AUDIT),
            Self::Stable => None,
        }
    }

    /// Tells implementing agents how much polish the stage expects; `None` adds nothing.
    pub fn work_hint(self) -> Option<&'static str> {
        match self {
            Self::Prototype => Some(
                "Project stage: prototype. Favor reaching a working idea quickly over completeness: \
                 rough edges and bugs off the main path are acceptable, but never lose data or weaken security.",
            ),
            Self::Experimental => Some(
                "Project stage: experimental. Keep the main path working and expand quickly: \
                 bugs on uncommon paths are acceptable follow-ups, but never lose data or weaken security.",
            ),
            Self::Stable => None,
        }
    }
}

const PROTOTYPE_AUDIT: &str = "Project stage: prototype (idea or draft). The project trades completeness for speed of exploration and tolerates bugs. This stage narrows the high rating above: rate high only a finding that loses or corrupts data, exposes credentials or breaks a security boundary, or leaves the change unable to do what it was made for. Wrong results off the main path, crashes on unusual input, missing tests, error handling and hardening are low follow-ups. Without such a high finding the verdict is SHIP.";

const EXPERIMENTAL_AUDIT: &str = "Project stage: experimental (development or experiment). The design is still moving and bugs on uncommon paths are tolerated. This stage narrows the high rating above: rate high only a finding that loses or corrupts data, exposes credentials or breaks a security boundary, or breaks a main path that worked before. Other wrong results and crashes are medium; missing tests, error handling and hardening are low. Without such a high finding the verdict is SHIP.";
