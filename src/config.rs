// Configuration loading stubs for the v0.5 foundation surface.
// Exports AidConfig and a no-op loader until config support is implemented.

pub struct AidConfig;

pub fn load_config() -> anyhow::Result<AidConfig> {
    Ok(AidConfig)
}
