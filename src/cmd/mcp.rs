// Handler for `aid mcp` foundation wiring.
// Provides a compile-safe placeholder until the MCP server is implemented.

use anyhow::Result;
use std::sync::Arc;

use crate::store::Store;

pub async fn run(_store: Arc<Store>) -> Result<()> {
    println!("MCP server not yet implemented");
    Ok(())
}
