// User interfaces and shared output control.
// Groups text boards, terminal dashboards, and the optional web server.

#[macro_use]
pub mod output;
pub(crate) mod board;
pub(crate) mod format;
pub(crate) mod tui;
#[cfg(feature = "web")]
pub(crate) mod web;
