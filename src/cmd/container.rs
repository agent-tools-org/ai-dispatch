// Handlers for `aid container` subcommands.
// Exports small wrappers around the shared container lifecycle helpers.
// Deps: crate::container, anyhow.

use anyhow::Result;

pub enum ContainerAction {
    Build { tag: String, file: Option<String> },
    List,
    Stop { name: String },
}

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
