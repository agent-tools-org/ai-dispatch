// Process resource sampling for running TUI tasks.
// Exports ProcessMetrics and `ps`-based collection helpers; depends on std::process.

use std::process::Command;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessMetrics {
    pub cpu_percent: f32,
    pub memory_mb: f32,
}

pub fn get_process_metrics(pid: u32) -> Option<ProcessMetrics> {
    let output = Command::new("ps")
        .args(["-o", "%cpu=,rss=", "-p", &pid.to_string()])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_ps_output(std::str::from_utf8(&output.stdout).ok()?)
}

pub(crate) fn parse_ps_output(output: &str) -> Option<ProcessMetrics> {
    let mut fields = output.split_whitespace();
    let cpu_percent = fields.next()?.parse().ok()?;
    let rss_kb: f32 = fields.next()?.parse().ok()?;
    Some(ProcessMetrics {
        cpu_percent,
        memory_mb: rss_kb / 1024.0,
    })
}

#[cfg(test)]
mod tests;
