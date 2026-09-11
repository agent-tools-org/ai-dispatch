// Embed release notes and version provenance at build time.
// Entry point: main; deps: std filesystem, environment, and git subprocess.
use std::process::Command;

const CHANGELOG_RELEASE_LIMIT: usize = 10;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn build_embedded_changelog() -> std::io::Result<String> {
    let content = std::fs::read_to_string("CHANGELOG.md")?;
    let mut releases = 0;
    Ok(content
        .split_inclusive('\n')
        .skip_while(|line| !line.starts_with("## v"))
        .take_while(|line| {
            if line.starts_with("## v") {
                releases += 1;
            }
            releases <= CHANGELOG_RELEASE_LIMIT
        })
        .collect())
}

/// Commit identity + dirty flag for `aid --version`. Falls back to a plain
/// "no git metadata" marker (never a blank field) when there is no git repo
/// to inspect, e.g. a crates.io tarball or released source archive.
fn build_git_info() -> String {
    git(&["describe", "--always", "--dirty"])
        .filter(|desc| !desc.is_empty())
        .unwrap_or_else(|| "no git metadata".to_string())
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Watch git locations so version provenance updates when tags change.
    println!("cargo:rerun-if-changed=.git/refs/tags");
    println!("cargo:rerun-if-changed=.git/packed-refs");
    println!("cargo:rerun-if-changed=Cargo.toml");
    println!("cargo:rerun-if-changed=CHANGELOG.md");
    // Watch HEAD/index so the embedded commit SHA and dirty flag stay current.
    println!("cargo:rerun-if-changed=.git/HEAD");
    println!("cargo:rerun-if-changed=.git/index");
    let text = build_embedded_changelog()?;
    let out_dir = std::env::var_os("OUT_DIR").ok_or("OUT_DIR is not set")?;
    std::fs::write(std::path::PathBuf::from(out_dir).join("changelog.txt"), text)?;
    println!("cargo:rustc-env=AID_GIT_INFO={}", build_git_info());
    Ok(())
}
