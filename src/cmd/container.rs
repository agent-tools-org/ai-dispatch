// Handlers for `aid container` subcommands.
// Exports small wrappers around the shared container lifecycle helpers.
// Deps: crate::runtime::container, anyhow.

use crate::cli::ContainerAction;
use anyhow::Result;

pub fn run_container_command(action: ContainerAction) -> Result<()> {
    match action {
        ContainerAction::Build { tag, file } => crate::runtime::container::build_image(&tag, file.as_deref()),
        ContainerAction::List => crate::runtime::container::list_containers(),
        ContainerAction::Stop { name } => {
            crate::runtime::container::stop_container(&name);
            Ok(())
        }
    }
}
