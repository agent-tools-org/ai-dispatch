// Antigravity authentication mount coverage independent of removed agent names.
// Deps: parent container mount helper and isolated temporary home.
use super::*;
use std::fs;
use tempfile::TempDir;

#[test]
fn agy_mounts_its_auth_directory_without_a_gemini_agent() {
    let home = TempDir::new().expect("home");
    fs::create_dir(home.path().join(".gemini")).expect("agy auth directory");
    let mounts = mount_home_dirs_from(Some(home.path().to_path_buf()));
    let expected = format!("{}:/root/.gemini", home.path().join(".gemini").display());
    assert!(
        mounts
            .windows(2)
            .any(|pair| pair == ["-v", expected.as_str()])
    );
    assert_eq!(crate::types::AgentKind::parse_str("gemini"), None);
    assert!(mount_home_dirs_from(None).is_empty());
}
