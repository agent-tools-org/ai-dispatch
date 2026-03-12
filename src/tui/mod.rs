// TUI entrypoint stub for the v0.5 foundation surface.
// Exports a no-op runner until the interactive dashboard is implemented.

pub fn run(_store: &std::sync::Arc<crate::store::Store>) -> anyhow::Result<()> {
    println!("TUI not yet implemented");
    Ok(())
}
