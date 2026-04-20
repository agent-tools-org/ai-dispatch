// GitButler-specific batch/watch helpers: enable prompt and merge-back hints.
// Exports prompt gating plus shared summary text for batch and quiet watch flows.
// Deps: crate::gitbutler, crate::project, anyhow, std::io/path.

use anyhow::Result;
use std::io::{self, IsTerminal, Write};
use std::path::Path;

pub(crate) fn maybe_prompt_gitbutler_batch_integration(
    repo_dir: &Path,
    no_prompt: bool,
) -> Result<()> {
    if !should_prompt_for_gitbutler_integration(repo_dir, no_prompt, interactive_stdio()) {
        return Ok(());
    }
    print!(
        "Detected GitButler repo without aid's gitbutler integration enabled.\nEnable now? (recommended) [Y/n]: "
    );
    io::stdout().flush()?;
    let mut input = String::new();
    io::stdin().read_line(&mut input)?;
    if accepts_gitbutler_prompt(&input) {
        let path = crate::project::upsert_gitbutler_mode(repo_dir, "auto")?;
        aid_info!("[aid] Enabled GitButler integration in {}", path.display());
    } else {
        let path = crate::project::upsert_gitbutler_prompt_suppressed(repo_dir, true)?;
        aid_info!("[aid] Suppressed future GitButler batch prompts in {}", path.display());
    }
    Ok(())
}

pub(crate) fn merge_back_hint(repo_dir: &Path, group_id: &str) -> Option<String> {
    if std::env::var("AID_GITBUTLER").is_ok_and(|value| value == "0") {
        return None;
    }
    let project = crate::project::detect_project_in(repo_dir)?;
    if matches!(project.gitbutler_mode(), crate::gitbutler::Mode::Off) {
        return None;
    }
    if !crate::gitbutler::but_available() {
        return None;
    }
    Some(format!(
        "To integrate into workspace:\n  aid merge --lanes --group {group_id}        # apply as GitButler lanes\n  aid merge --group {group_id}                 # merge into current branch"
    ))
}

pub(crate) fn should_prompt_for_gitbutler_integration(
    repo_dir: &Path,
    no_prompt: bool,
    interactive: bool,
) -> bool {
    if no_prompt || !interactive {
        return false;
    }
    if !crate::gitbutler::but_available() || !crate::gitbutler::repo_has_markers(repo_dir) {
        return false;
    }
    match crate::project::detect_project_in(repo_dir) {
        Some(project) => project.gitbutler.is_none() && !project.suppress_gitbutler_prompt,
        None => true,
    }
}

fn interactive_stdio() -> bool {
    io::stdin().is_terminal() && io::stdout().is_terminal()
}

fn accepts_gitbutler_prompt(input: &str) -> bool {
    matches!(input.trim().to_ascii_lowercase().as_str(), "" | "y" | "yes")
}

#[cfg(test)]
mod tests {
    use super::{merge_back_hint, should_prompt_for_gitbutler_integration};
    use std::fs;
    use std::sync::{Mutex, MutexGuard, OnceLock};

    fn env_lock() -> MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        match LOCK.get_or_init(|| Mutex::new(())).lock() {
            Ok(guard) => guard,
            Err(poison) => poison.into_inner(),
        }
    }

    struct EnvGuard {
        key: &'static str,
        original: Option<String>,
    }

    impl EnvGuard {
        fn set(key: &'static str, value: &'static str) -> Self {
            let original = std::env::var(key).ok();
            unsafe { std::env::set_var(key, value) };
            Self { key, original }
        }
    }

    impl Drop for EnvGuard {
        fn drop(&mut self) {
            match self.original.as_deref() {
                Some(value) => unsafe { std::env::set_var(self.key, value) },
                None => unsafe { std::env::remove_var(self.key) },
            }
        }
    }

    #[test]
    fn prompt_check_requires_missing_gitbutler_setting() {
        let _guard = env_lock();
        let repo = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success());
        let _but = EnvGuard::set("AID_GITBUTLER_TEST_PRESENT", "1");
        let _repo = EnvGuard::set("AID_GITBUTLER_TEST_REPO_MARKERS", "1");
        let project_path = repo.path().join(".aid/project.toml");
        fs::create_dir_all(project_path.parent().unwrap()).unwrap();
        fs::write(&project_path, "[project]\nid = \"demo\"\ngitbutler = \"auto\"\n").unwrap();

        assert!(!should_prompt_for_gitbutler_integration(
            repo.path(),
            false,
            true
        ));
    }

    #[test]
    fn prompt_check_skips_when_suppressed() {
        let _guard = env_lock();
        let repo = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success());
        let _but = EnvGuard::set("AID_GITBUTLER_TEST_PRESENT", "1");
        let _repo = EnvGuard::set("AID_GITBUTLER_TEST_REPO_MARKERS", "1");
        let project_path = repo.path().join(".aid/project.toml");
        fs::create_dir_all(project_path.parent().unwrap()).unwrap();
        fs::write(
            &project_path,
            "[project]\nid = \"demo\"\nsuppress_gitbutler_prompt = true\n",
        )
        .unwrap();

        assert!(!should_prompt_for_gitbutler_integration(
            repo.path(),
            false,
            true
        ));
    }

    #[test]
    fn merge_hint_requires_active_project_mode() {
        let _guard = env_lock();
        let repo = tempfile::tempdir().unwrap();
        assert!(std::process::Command::new("git")
            .arg("-C")
            .arg(repo.path())
            .args(["init", "-q"])
            .status()
            .unwrap()
            .success());
        let _but = EnvGuard::set("AID_GITBUTLER_TEST_PRESENT", "1");
        let project_path = repo.path().join(".aid/project.toml");
        fs::create_dir_all(project_path.parent().unwrap()).unwrap();
        fs::write(&project_path, "[project]\nid = \"demo\"\ngitbutler = \"auto\"\n").unwrap();

        let hint = merge_back_hint(repo.path(), "wg-demo").unwrap();
        assert!(hint.contains("aid merge --lanes --group wg-demo"));
        assert!(hint.contains("aid merge --group wg-demo"));
    }
}
