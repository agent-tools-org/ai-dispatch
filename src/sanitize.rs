// Input validation and path safety for user-supplied identifiers.
// Prevents path traversal, command injection, and sandbox escapes.
// All user-controlled IDs must pass through these validators before filesystem use.

use anyhow::{bail, Result};
use std::path::{Path, PathBuf};

/// Validate a task ID: must match `t-[0-9a-f]{4}` pattern.
pub fn validate_task_id(id: &str) -> Result<()> {
    if id.len() >= 3
        && id.starts_with("t-")
        && id[2..].chars().all(|c| c.is_ascii_hexdigit())
        && id.len() <= 6
    {
        return Ok(());
    }
    bail!("Invalid task ID '{id}': must match t-XXXX (hex)")
}

/// Validate a workgroup ID: must start with `wg-` followed by safe characters.
/// Accepts both generated hex IDs (wg-a3f1) and custom names (wg-my-feature).
pub fn validate_workgroup_id(id: &str) -> Result<()> {
    if id.len() < 4 || !id.starts_with("wg-") {
        bail!("Invalid workgroup ID '{id}': must start with 'wg-'");
    }
    let suffix = &id[3..];
    if suffix.is_empty() {
        bail!("Invalid workgroup ID '{id}': empty suffix");
    }
    if suffix.contains('/') || suffix.contains('\\') || suffix.contains("..") {
        bail!("Invalid workgroup ID '{id}': path separators and '..' are forbidden");
    }
    if !suffix
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        bail!("Invalid workgroup ID '{id}': only alphanumeric, '-', '_' allowed after 'wg-'");
    }
    Ok(())
}

/// Validate an identifier used as a filesystem component (agent name, team name,
/// skill name, template name). Must be alphanumeric with hyphens/underscores/dots.
/// No path separators, no `..`, no leading dash.
pub fn validate_name(name: &str, kind: &str) -> Result<()> {
    if name.is_empty() {
        bail!("Empty {kind} name");
    }
    if name.starts_with('-') || name.starts_with('.') {
        bail!("Invalid {kind} name '{name}': must not start with '-' or '.'");
    }
    if name.contains('/') || name.contains('\\') || name.contains("..") {
        bail!("Invalid {kind} name '{name}': path separators and '..' are forbidden");
    }
    if !name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_' || c == '.')
    {
        bail!(
            "Invalid {kind} name '{name}': only alphanumeric, '-', '_', '.' allowed"
        );
    }
    Ok(())
}

/// Validate a git branch name for worktree use.
/// Allows `/` for namespaced branches (feat/foo) but rejects `..`, leading `-`,
/// and characters unsafe for both git refs and filesystem paths.
pub fn validate_branch_name(branch: &str) -> Result<()> {
    if branch.is_empty() {
        bail!("Empty branch name");
    }
    if branch.starts_with('-') {
        bail!("Invalid branch name '{branch}': must not start with '-'");
    }
    if branch.contains("..") {
        bail!("Invalid branch name '{branch}': '..' is forbidden");
    }
    if branch.contains('~') || branch.contains('^') || branch.contains(':') {
        bail!("Invalid branch name '{branch}': git revision syntax characters forbidden");
    }
    if branch.contains('\0') || branch.contains(' ') || branch.contains('\\') {
        bail!("Invalid branch name '{branch}': contains unsafe characters");
    }
    // Reject shell metacharacters
    for c in [';', '|', '&', '$', '`', '(', ')', '{', '}', '<', '>', '!', '*', '?'] {
        if branch.contains(c) {
            bail!("Invalid branch name '{branch}': shell metacharacter '{c}' forbidden");
        }
    }
    Ok(())
}

/// Join a user-supplied component under a base directory and verify containment.
/// Returns the normalized path. Rejects traversal attempts.
pub fn safe_join(base: &Path, component: &str) -> Result<PathBuf> {
    // Quick reject: component must not contain `..`
    if component.contains("..") {
        bail!(
            "Path traversal blocked: '{}' contains '..'",
            component
        );
    }
    let joined = base.join(component);
    let normalized = normalize_path(&joined);
    let normalized_base = normalize_path(base);
    if !normalized.starts_with(&normalized_base) {
        bail!(
            "Path traversal blocked: '{}' escapes base '{}'",
            component,
            base.display()
        );
    }
    Ok(normalized)
}

/// Normalize a path without requiring it to exist (resolve `.` and `..` components).
fn normalize_path(path: &Path) -> PathBuf {
    let mut components = Vec::new();
    for component in path.components() {
        match component {
            std::path::Component::ParentDir => {
                components.pop();
            }
            std::path::Component::CurDir => {}
            other => components.push(other),
        }
    }
    components.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn valid_task_ids() {
        assert!(validate_task_id("t-a3f1").is_ok());
        assert!(validate_task_id("t-0000").is_ok());
        assert!(validate_task_id("t-ffff").is_ok());
    }

    #[test]
    fn invalid_task_ids() {
        assert!(validate_task_id("").is_err());
        assert!(validate_task_id("t-").is_err());
        assert!(validate_task_id("t-ZZZZ").is_err());
        assert!(validate_task_id("../etc").is_err());
        assert!(validate_task_id("t-a3f1/../../etc").is_err());
        assert!(validate_task_id("t-a3f1a3f1").is_err());
    }

    #[test]
    fn valid_workgroup_ids() {
        assert!(validate_workgroup_id("wg-a3f1").is_ok());
        assert!(validate_workgroup_id("wg-0000").is_ok());
        assert!(validate_workgroup_id("wg-custom").is_ok());
        assert!(validate_workgroup_id("wg-my-feature").is_ok());
        assert!(validate_workgroup_id("wg-shared").is_ok());
    }

    #[test]
    fn invalid_workgroup_ids() {
        assert!(validate_workgroup_id("").is_err());
        assert!(validate_workgroup_id("wg-").is_err());
        assert!(validate_workgroup_id("../../etc").is_err());
        assert!(validate_workgroup_id("wg-a3f1/../../x").is_err());
        assert!(validate_workgroup_id("wg-foo/../bar").is_err());
    }

    #[test]
    fn valid_names() {
        assert!(validate_name("codex", "agent").is_ok());
        assert!(validate_name("my-agent", "agent").is_ok());
        assert!(validate_name("test_writer", "skill").is_ok());
        assert!(validate_name("v1.2", "agent").is_ok());
    }

    #[test]
    fn invalid_names() {
        assert!(validate_name("", "agent").is_err());
        assert!(validate_name("-leading", "agent").is_err());
        assert!(validate_name(".hidden", "agent").is_err());
        assert!(validate_name("../escape", "agent").is_err());
        assert!(validate_name("foo/bar", "agent").is_err());
        assert!(validate_name("foo\\bar", "agent").is_err());
        assert!(validate_name("foo bar", "agent").is_err());
        assert!(validate_name("foo;rm -rf", "agent").is_err());
    }

    #[test]
    fn valid_branch_names() {
        assert!(validate_branch_name("feat/my-feature").is_ok());
        assert!(validate_branch_name("fix/bug-123").is_ok());
        assert!(validate_branch_name("main").is_ok());
        assert!(validate_branch_name("v1.0.0").is_ok());
    }

    #[test]
    fn invalid_branch_names() {
        assert!(validate_branch_name("").is_err());
        assert!(validate_branch_name("-flag").is_err());
        assert!(validate_branch_name("feat/../escape").is_err());
        assert!(validate_branch_name("branch;rm -rf /").is_err());
        assert!(validate_branch_name("branch$(cmd)").is_err());
        assert!(validate_branch_name("HEAD^{commit}").is_err());
        assert!(validate_branch_name("main~1").is_err());
    }

    #[test]
    fn safe_join_blocks_traversal() {
        let base = PathBuf::from("/tmp");
        assert!(safe_join(&base, "good-dir").is_ok());
        assert!(safe_join(&base, "../etc/passwd").is_err());
        assert!(safe_join(&base, "foo/../../etc").is_err());
    }

    #[test]
    fn safe_join_allows_nested() {
        let base = PathBuf::from("/tmp");
        let result = safe_join(&base, "aid-wt-feat/subdir").unwrap();
        assert!(result.starts_with("/tmp") || result.starts_with("/private/tmp"));
    }
}
