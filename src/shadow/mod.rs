// Shadow classification: record what a System One model would decide, without acting on it.
// Exports: secret (key access and the authenticated call).
// Deps: curl on PATH; the macOS login keychain for the key.

pub(crate) mod secret;
