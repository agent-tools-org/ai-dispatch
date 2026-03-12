// Handler for `aid config` foundation wiring.
// Provides a compile-safe placeholder until persistent config support lands.

use anyhow::Result;
use std::sync::Arc;

use crate::store::Store;

pub fn run(_store: &Arc<Store>, _action: crate::ConfigAction) -> Result<()> {
    println!("Config not yet implemented");
    Ok(())
}
