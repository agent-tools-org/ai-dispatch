// Identity probes use stable environments and the Linux kernel start-time field.
// Recording and checking share root_started rather than formatting dates independently.
use super::*;

#[test]
fn caller_owned_identity_command_pins_timezone_and_locale() {
    let command = identity_command(std::process::id());
    let env = command.get_envs().collect::<Vec<_>>();
    assert!(env.contains(&(std::ffi::OsStr::new("TZ"), Some(std::ffi::OsStr::new("UTC0")))));
    assert!(env.contains(&(std::ffi::OsStr::new("LC_ALL"), Some(std::ffi::OsStr::new("C")))));
}

#[cfg(target_os = "linux")]
#[test]
fn caller_owned_linux_identity_is_proc_starttime_field_22() {
    let pid = std::process::id();
    let stat = std::fs::read_to_string(format!("/proc/{pid}/stat")).expect("stat");
    let start = stat
        .rsplit_once(") ")
        .expect("comm")
        .1
        .split_whitespace()
        .nth(19)
        .expect("field 22");
    assert_eq!(root_started(pid).as_deref(), Some(start));
    assert!(root_matches(pid, Some(start)));
}
