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

#[test]
fn every_start_time_ps_command_uses_the_shared_environment() {
    for command in [identity_command(42), table_command()] {
        assert_eq!(command.get_program(), "ps");
        let env = command.get_envs().collect::<Vec<_>>();
        for (key, value) in [("TZ", "UTC0"), ("LC_ALL", "C")] {
            assert!(env.contains(&(std::ffi::OsStr::new(key), Some(std::ffi::OsStr::new(value)))));
        }
        assert!(command.get_args().any(|arg| arg == "lstart="));
    }
    let source = include_str!("activity.rs");
    assert_eq!(source.matches("Command::new(\"ps\")").count(), 1);
    for source in [include_str!("../group.rs"), include_str!("../../execution_probe.rs")] {
        assert!(!source.contains("Command::new(\"ps\")"));
        assert!(source.contains("activity::ps_command()"));
    }
}

#[cfg(target_os = "linux")]
#[test]
fn process_table_and_root_identity_use_the_same_proc_starttime() {
    let pid = std::process::id();
    let table = process_table().expect("process table");
    let process = table.iter().find(|process| process.pid == pid).expect("self");
    assert_eq!(Some(process.started.clone()), root_started(pid));
}
