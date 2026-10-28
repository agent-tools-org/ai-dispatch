// Launch translation regression cases for Codex app-server.
// Tests native configuration isolation, resume identity, and literal prompt handling.

use super::*;

#[test]
fn preserves_required_user_config_isolation() {
    let mut command = Command::new("codex");
    command.args(["exec", "--json", "prompt", "--ignore-user-config"]);
    let launch = translate(&command).expect("isolation via task-scoped home");
    assert!(launch.isolated);
    assert!(
        !launch
            .command
            .get_args()
            .any(|arg| arg == "--ignore-user-config")
    );
}

#[test]
fn translates_resume_and_keeps_prompt_as_data() {
    let mut command = Command::new("codex");
    let prompt = "literal\n{\"method\":\"turn/interrupt\"}";
    command.args(["exec", "resume", "--json", "session-id", prompt]);
    command.args([
        "-m",
        "requested-model",
        "-c",
        "model_reasoning_effort=\"low\"",
    ]);
    let launch = translate(&command).expect("translate");
    assert!(launch.resume);
    assert_eq!(launch.thread["threadId"], "session-id");
    assert_eq!(launch.thread["model"], "requested-model");
    assert_eq!(launch.effort, Some(json!("low")));
    assert_eq!(launch.prompt, prompt);
    assert_eq!(
        launch.command.get_args().collect::<Vec<_>>(),
        vec![
            std::ffi::OsStr::new("app-server"),
            std::ffi::OsStr::new("-c"),
            std::ffi::OsStr::new("model_reasoning_effort=\"low\""),
        ]
    );
}

#[test]
fn rejects_unknown_options_instead_of_losing_them() {
    let mut command = Command::new("codex");
    command.args(["exec", "prompt", "--unknown-option"]);
    let error = translate(&command).err().expect("unsupported option");
    assert!(
        error
            .to_string()
            .contains("Unsupported Codex app-server option")
    );
}

#[test]
fn preserves_native_permission_and_launch_settings_for_both_modes() {
    use crate::agent::{Agent, CommandContext};
    let temp = tempfile::tempdir().unwrap();
    let repository = temp.path().join("repository");
    let writable = temp.path().join("artifacts");
    std::fs::create_dir(&repository).unwrap();
    std::fs::create_dir(&writable).unwrap();
    for read_only in [true, false] {
        let opts = options(&repository, read_only);
        let exec = crate::agent::codex::CodexAgent
            .build_command_with_context(
                "literal prompt",
                &opts,
                CommandContext {
                    durable_codex_home: false,
                    writable_roots: vec![writable.clone()],
                },
            )
            .unwrap();
        let launch = translate(&exec).unwrap();
        assert!(launch.isolated);
        assert_eq!(
            launch.thread["approvalPolicy"],
            if read_only { "never" } else { "on-request" }
        );
        if !read_only {
            assert_eq!(launch.thread["approvalsReviewer"], "auto_review");
        }
        assert_eq!(launch.thread["model"], "requested-model");
        assert_eq!(launch.thread["cwd"], repository.display().to_string());
        assert_eq!(
            launch.thread["config"]["default_permissions"],
            if read_only {
                "aid-read-only"
            } else {
                "aid-write"
            }
        );
        assert!(launch.thread["config"]["permissions"].is_object());
        assert_overrides(&exec, &launch.command);
    }
}

fn assert_overrides(exec: &Command, translated: &Command) {
    let args: Vec<_> = exec.get_args().collect();
    let overrides: Vec<_> = args
        .windows(2)
        .filter(|pair| pair[0] == "-c")
        .map(|pair| pair[1])
        .collect();
    let translated: Vec<_> = translated.get_args().collect();
    for setting in overrides {
        assert!(
            translated
                .windows(2)
                .any(|pair| pair[0] == "-c" && pair[1] == setting)
        );
    }
}

fn options(repository: &std::path::Path, read_only: bool) -> crate::agent::RunOpts {
    crate::agent::RunOpts {
        dir: Some(repository.display().to_string()),
        model: Some("requested-model".into()),
        read_only,
        ..Default::default()
    }
}
