// Handler for `aid respond` input forwarding.
// Writes one response payload for a background task under ~/.aid/jobs.

use anyhow::Result;

use crate::input_signal;

pub fn run(task_id: &str, input: &str) -> Result<()> {
    input_signal::write_response(task_id, input)?;
    println!("Queued input for {task_id}");
    Ok(())
}
