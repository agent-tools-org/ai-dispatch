// Shared Git stdout capture for workspace operations.
// Exports checked raw and trimmed output; depends on anyhow and the Git CLI.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

pub(crate) fn git_output(dir: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(args)
        .output()
        .with_context(|| format!("Failed to run git {}", args.join(" ")))?;
    anyhow::ensure!(
        output.status.success(),
        "git {} failed: {}",
        args.join(" "),
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).to_string())
}

pub(crate) fn git_output_trimmed(dir: &Path, args: &[&str]) -> Result<String> {
    Ok(git_output(dir, args)?.trim().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_output_preserves_whitespace_and_trimmed_output_removes_it() {
        let _permit = crate::support::test_subprocess::acquire();
        let repo = tempfile::tempdir().unwrap();
        git_output(repo.path(), &["init"]).unwrap();
        let args = ["rev-parse", "--is-inside-work-tree"];
        assert_eq!(git_output(repo.path(), &args).unwrap(), "true\n");
        assert_eq!(git_output_trimmed(repo.path(), &args).unwrap(), "true");
    }

    #[test]
    fn failures_include_arguments_and_trimmed_stderr() {
        let _permit = crate::support::test_subprocess::acquire();
        let repo = tempfile::tempdir().unwrap();
        let args = ["rev-parse", "--is-inside-work-tree"];
        let output = Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        let expected = format!(
            "git {} failed: {}",
            args.join(" "),
            String::from_utf8_lossy(&output.stderr).trim()
        );
        assert_eq!(
            git_output(repo.path(), &args).unwrap_err().to_string(),
            expected
        );
        assert_eq!(
            git_output_trimmed(repo.path(), &args)
                .unwrap_err()
                .to_string(),
            expected
        );
    }
}
