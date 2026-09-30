// aid CLI admin and configuration dispatch handlers.
// Implements config, local tools, setup, upgrade, and web wrappers.

use crate::cli::{ByokCommands, HookAction, StoreCommands};
use crate::cli::{ConfigAction, ContainerAction, CredentialAction, TeamAction, ToolAction};
use crate::cmd;
use crate::store;
use anyhow::Result;
use std::sync::Arc;

pub(super) async fn mcp(store: Arc<store::Store>) -> Result<()> {
    cmd::mcp::run(store).await
}

pub(super) fn hook(action: HookAction) -> Result<()> {
    match action {
        HookAction::SessionStart => cmd::hook::session_start(),
    }
}

pub(super) fn config(store: Arc<store::Store>, action: ConfigAction) -> Result<()> {
    cmd::config::run(&store, action)
}

pub(super) fn container(action: ContainerAction) -> Result<()> {
    cmd::container::run_container_command(action)
}

pub(super) fn store(action: StoreCommands) -> Result<()> {
    cmd::store::run_store(action)
}

pub(super) fn team(action: TeamAction) -> Result<()> {
    cmd::team::run_team_command(action)
}

pub(super) fn tool(action: ToolAction) -> Result<()> {
    cmd::tool::run_tool_command(action)
}

pub(super) fn byok(action: ByokCommands) -> Result<()> {
    cmd::byok::run_byok_command(action)
}

pub(super) fn credential(action: CredentialAction) -> Result<()> {
    cmd::credential::run_credential_command(action)
}

pub(super) fn upgrade(force: bool) -> Result<()> {
    cmd::upgrade::run(force)
}

#[cfg(feature = "web")]
pub(super) async fn run_web(port: u16, host: String, token: Option<String>) -> Result<()> {
    cmd::web::run(port, host, token).await
}

pub(super) fn init() -> Result<()> {
    cmd::init::run()
}

pub(super) fn setup() -> Result<()> {
    cmd::setup::run()
}
