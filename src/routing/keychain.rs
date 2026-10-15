// Reads an API key from the macOS login keychain and hands it to one child process.
// Exports: ApiKey, KeychainItem, read_api_key(), ApiKey::apply_to().
// Deps: std::process::Command, anyhow. The key never implements Debug or Display.

use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;

const SECURITY: &str = "/usr/bin/security";

/// Where a route's key lives: `security find-generic-password -s <service> -a <account>`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KeychainItem {
    pub service: String,
    pub account: String,
}

/// A secret that can only be moved into a child environment. No Debug, Display or Clone,
/// so it cannot reach logs, events, errors or saved dispatch arguments by formatting.
pub(crate) struct ApiKey(String);

impl ApiKey {
    /// Set `env_name` on `cmd` only; the dispatcher's own environment is untouched.
    pub(crate) fn apply_to(self, cmd: &mut Command, env_name: &str) {
        cmd.env(env_name, self.0);
    }
}

pub(crate) fn read_api_key(item: &KeychainItem) -> Result<ApiKey> {
    read_with(Path::new(SECURITY), item)
}

fn read_with(program: &Path, item: &KeychainItem) -> Result<ApiKey> {
    let output = Command::new(program)
        .args(["find-generic-password", "-s", &item.service, "-a", &item.account, "-w"])
        .output()
        .with_context(|| format!("cannot run {} for keychain item {}/{}",
            program.display(), item.service, item.account))?;
    if !output.status.success() {
        // stderr names the item, never the secret; it is still left out of the error.
        bail!("keychain item {}/{} is not readable (security exit {})",
            item.service, item.account, output.status.code().unwrap_or(-1));
    }
    let Ok(value) = String::from_utf8(output.stdout) else {
        bail!("keychain item {}/{} is not UTF-8", item.service, item.account);
    };
    let value = value.trim_end_matches(['\n', '\r']);
    if value.is_empty() || value.chars().any(char::is_whitespace) {
        bail!("keychain item {}/{} does not hold a single-line key", item.service, item.account);
    }
    Ok(ApiKey(value.to_string()))
}

#[cfg(test)]
#[path = "keychain_tests.rs"]
mod tests;
