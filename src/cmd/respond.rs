// Handler for `aid respond` input forwarding.
// Writes one response payload for a background task under ~/.aid/jobs.

use anyhow::{Context, Result};

use crate::input_signal;

pub fn run(task_id: &str, input: Option<&str>, file: Option<&str>) -> Result<()> {
    let text = if let Some(path) = file {
        std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read response file: {path}"))?
    } else if let Some(text) = input {
        text.to_string()
    } else {
        use std::io::Read;

        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("Failed to read from stdin")?;
        buf
    };
    input_signal::write_response(task_id, &text)?;
    println!("Queued input for {task_id}");
    Ok(())
}
