// TypeSafe System One access for aid: the key, the authenticated call, and typed questions.
// Exports: secret (key access and the authenticated call), classify (the shared request path),
// question (flag pairing and validation), screen (state checks), answer (response validation).
// Deps: curl on PATH; the macOS login keychain for the key.

mod answer;
pub(crate) mod classify;
pub(crate) mod question;
mod screen;
pub(crate) mod secret;
