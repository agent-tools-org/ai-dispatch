// Clean's payload-retention reporting and daily post-task maintenance.
// Uses the existing update-check state file for cadence and resumable row cursors.
use crate::{
    store::Store,
    support::{paths, update_check::with_cache_state},
};
use anyhow::{Result, bail};
use chrono::{Duration as Days, Utc};
use std::time::Duration;

pub(super) fn run(store: &Store, days: u64, dry_run: bool, vacuum: bool) -> Result<()> {
    let cutoff = cutoff(days)?;
    let path = paths::aid_dir().join("update-check.json");
    let report = if dry_run {
        Some(store.trim_event_payloads(&cutoff, 0, true, Duration::from_secs(10))?)
    } else {
        with_cache_state(&path, |state| {
            let cursor = if state["payload_clean_days"].as_u64() == Some(days) {
                state["payload_clean_cursor"].as_i64().unwrap_or(0)
            } else {
                0
            };
            let report =
                store.trim_event_payloads(&cutoff, cursor, false, Duration::from_secs(10))?;
            state["payload_clean_cursor"] = report.cursor.into();
            state["payload_clean_days"] = days.into();
            Ok(Some(report))
        })?
    };
    let Some(report) = report else {
        bail!("Payload maintenance is already running; retry aid clean")
    };
    println!(
        "{} {} event rows ({} payload bytes removed){}",
        if dry_run {
            "[dry-run] Would trim"
        } else {
            "Trimmed"
        },
        report.rows,
        report.bytes,
        if report.complete {
            ""
        } else {
            "; budget reached, run aid clean again to continue"
        }
    );
    if vacuum {
        compact(store, dry_run)?;
    }
    Ok(())
}

fn compact(store: &Store, dry_run: bool) -> Result<()> {
    if dry_run {
        println!("[dry-run] Would VACUUM after trimming (requires idle tasks and free disk space)");
    } else {
        let (before, after) = store.vacuum_events()?;
        println!(
            "VACUUM: {} → {}",
            super::format_bytes(before),
            super::format_bytes(after)
        );
    }
    Ok(())
}

pub(crate) fn auto_trim(store: &Store) {
    if let Err(error) = auto_trim_at(
        store,
        &paths::aid_dir().join("update-check.json"),
        Utc::now().timestamp(),
    ) {
        crate::aid_warn!("[aid] Payload maintenance deferred: {error}");
    }
}

fn auto_trim_at(store: &Store, path: &std::path::Path, now: i64) -> Result<()> {
    with_cache_state(path, |state| {
        if state["payload_trim_last_run"]
            .as_i64()
            .is_some_and(|last| now - last < 86400)
        {
            return Ok(None);
        }
        let cursor = state["payload_trim_cursor"].as_i64().unwrap_or(0);
        let report =
            store.trim_event_payloads(&cutoff(7)?, cursor, false, Duration::from_secs(2))?;
        state["payload_trim_last_run"] = now.into();
        state["payload_trim_cursor"] = report.cursor.into();
        Ok(Some(()))
    })?;
    Ok(())
}

fn cutoff(days: u64) -> Result<String> {
    let duration = i64::try_from(days)
        .ok()
        .and_then(Days::try_days)
        .ok_or_else(|| anyhow::anyhow!("--older-than is too large"))?;
    Ok(Utc::now()
        .checked_sub_signed(duration)
        .ok_or_else(|| anyhow::anyhow!("--older-than is out of range"))?
        .to_rfc3339())
}

#[cfg(test)]
#[path = "clean_retention_tests.rs"]
mod tests;
