// Repeated-poll notices for aid board.
// Deps: parent board poll state and task fingerprint helpers.

use super::*;

pub(super) fn board_poll_notice(
    tasks: &[Task],
    marker_path: &std::path::Path,
    fingerprint: &str,
    now: i64,
    force: bool,
    json: bool,
) -> (u32, ForceMarkerState, Option<String>, bool) {
    let mut repeat_count = 0;
    let mut force_state = ForceMarkerState::default();
    let mut poll_notice = None;
    let mut poll_failed = false;
    if !json {
        let anti_poll = anti_poll_status(&marker_path, &fingerprint, now, force);
        force_state = anti_poll.1;
        let watch_hint = watch_instead_of_polling_hint(&tasks);
        repeat_count = match anti_poll.0 {
            AntiPollStatus::Allowed(repeat_count) => repeat_count,
            AntiPollStatus::Cooldown(elapsed) => {
                poll_notice = Some(format!("[aid] Board checked {elapsed}s ago. {watch_hint}"));
                0
            }
            AntiPollStatus::Repeat(repeat_count) => {
                poll_notice = Some(format!(
                    "[aid] No changes after {repeat_count} checks. {watch_hint} Exiting."
                ));
                poll_failed = true;
                repeat_count
            }
            AntiPollStatus::ForceBlocked => {
                poll_notice = Some(format!(
                    "[aid] Repeated polling detected. Board locked for 60s. {watch_hint}"
                ));
                poll_failed = true;
                0
            }
        };
    }
    (repeat_count, force_state, poll_notice, poll_failed)
}

pub(super) fn record_board_poll(
    json: bool,
    marker_path: &std::path::Path,
    fingerprint: &str,
    now: i64,
    repeat_count: u32,
    force_state: ForceMarkerState,
) {
    if !json {
        write_board_marker(
            &marker_path,
            &fingerprint,
            now,
            repeat_count,
            force_state.count,
            force_state.window_start,
        );
    }
}
