// Handlers for `aid container` subcommands.
// Exports small wrappers around the shared container lifecycle helpers.
// Deps: crate::container, anyhow.

use crate::cli_actions::ContainerAction;
use anyhow::Result;

pub fn run_container_command(action: ContainerAction) -> Result<()> {
    match action {
        ContainerAction::Build { tag, file } => crate::container::build_image(&tag, file.as_deref()),
        ContainerAction::List => crate::container::list_containers(),
        ContainerAction::Stop { name } => {
            crate::container::stop_container(&name);
            Ok(())
        }
    }
}
