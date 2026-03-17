// CLI handler for `aid web` — starts the local web UI server.
// Exports: run; depends on crate::web, crate::store.

use anyhow::Result;
use std::sync::Arc;
use crate::store::Store;

pub async fn run(port: u16) -> Result<()> {
    let store = Arc::new(Store::open(&crate::paths::db_path())?);
    crate::web::serve(store, port).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_run_signature_compiles() {
        let _ = run;
    }
}
