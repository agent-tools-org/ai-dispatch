// Shared filesystem, validation, resource, and CLI support modules.
// Test helpers remain available only in test builds.

pub(crate) mod paths;
pub(crate) mod sanitize;
#[cfg(test)]
pub(crate) mod test_env;
#[cfg(test)]
pub(crate) mod test_subprocess;
pub(crate) mod repo_root;
pub(crate) mod shared_dir;
pub(crate) mod system_resources;
pub(crate) mod explore;
pub(crate) mod update_check;
