// System resource helpers for batch dispatch defaults and disk safety checks.
// Exports: cpu_count(), recommended_max_concurrent(), available_disk_mb(), check_disk_space()
// Deps: std::process::Command, std::thread
use std::process::Command;

pub fn cpu_count() -> usize {
    std::thread::available_parallelism()
        .map(usize::from)
        .unwrap_or(4)
}

pub fn recommended_max_concurrent() -> usize {
    (cpu_count() / 2).clamp(2, 16)
}

pub fn available_disk_mb(path: &str) -> Option<u64> {
    let output = Command::new("df").arg("-m").arg(path).output().ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .skip(1)
        .find_map(parse_available_mb)
}

#[cfg(test)]
pub fn check_disk_space(path: &str, min_mb: u64) -> bool {
    available_disk_mb(path).is_some_and(|available| available >= min_mb)
}

fn parse_available_mb(line: &str) -> Option<u64> {
    line.split_whitespace().nth(3)?.parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recommended_max_concurrent_is_in_range() {
        assert!((2..=16).contains(&recommended_max_concurrent()));
    }

    #[test]
    fn cpu_count_is_positive() {
        assert!(cpu_count() > 0);
    }

    #[test]
    fn available_disk_mb_returns_tmp_capacity() {
        assert!(available_disk_mb("/tmp").is_some());
    }

    #[test]
    fn check_disk_space_handles_zero_threshold() {
        assert!(check_disk_space("/tmp", 0));
    }

    #[test]
    fn parse_available_mb_reads_df_line() {
        assert_eq!(parse_available_mb("/dev/disk1 1024 512 512 50% /tmp"), Some(512));
    }
}
