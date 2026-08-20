// aid CLI admin and configuration dispatch handlers.
// Implements config, local tools, setup, upgrade, and web wrappers.

use crate::cli::{ByokCommands, HookAction, StoreCommands};
use crate::cli_actions::{ConfigAction, ContainerAction, CredentialAction, TeamAction, ToolAction};
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
    use cmd::container::{ContainerAction as ContainerCommand, run_container_command};
    let action = match action {
        ContainerAction::Build { tag, file } => ContainerCommand::Build { tag, file },
        ContainerAction::List => ContainerCommand::List,
        ContainerAction::Stop { name } => ContainerCommand::Stop { name },
    };
    run_container_command(action)
}

pub(super) fn store(action: StoreCommands) -> Result<()> {
    use cmd::store::{StoreAction, run_store};
    let action = match action {
        StoreCommands::Browse { query } => StoreAction::Browse { query },
        StoreCommands::Install { name } => StoreAction::Install { name },
        StoreCommands::Show { name } => StoreAction::Show { name },
        StoreCommands::Update { apply } => StoreAction::Update { apply },
    };
    run_store(action)
}

pub(super) fn team(action: TeamAction) -> Result<()> {
    use cmd::team::{TeamAction as TeamCommand, run_team_command};
    let action = match action {
        TeamAction::List => TeamCommand::List,
        TeamAction::Show { name } => TeamCommand::Show { name },
        TeamAction::Create { name } => TeamCommand::Create { name },
        TeamAction::Delete { name } => TeamCommand::Delete { name },
    };
    run_team_command(action)
}

pub(super) fn tool(action: ToolAction) -> Result<()> {
    cmd::tool::run_tool_command(action)
}

pub(super) fn byok(action: ByokCommands) -> Result<()> {
    use cmd::byok::{ByokAction, run_byok_command};
    let action = match action {
        ByokCommands::Apply { manifest, dry_run, key } => ByokAction::Apply { manifest, dry_run, key },
        ByokCommands::Remove { target } => ByokAction::Remove { target },
        ByokCommands::Probe { manifest, key } => ByokAction::Probe { manifest, key },
        ByokCommands::Example => ByokAction::Example,
        ByokCommands::Doc => ByokAction::Doc,
    };
    run_byok_command(action)
}

pub(super) fn credential(action: CredentialAction) -> Result<()> {
    use cmd::credential::{CredentialAction as CredentialCommand, run_credential_command};
    let action = match action {
        CredentialAction::List => CredentialCommand::List,
        CredentialAction::Add { provider, name, env } => CredentialCommand::Add { provider, name, env },
        CredentialAction::Remove { provider, name } => CredentialCommand::Remove { provider, name },
    };
    run_credential_command(action)
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
