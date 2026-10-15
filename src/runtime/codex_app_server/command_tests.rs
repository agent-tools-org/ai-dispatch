// Launch translation regression cases for the unwired app-server draft.
// Tests native configuration isolation, resume identity, and literal prompt handling.

use super::*;

#[test]
fn refuses_to_drop_required_user_config_isolation() {
    let mut command = Command::new("codex");
    command.args(["exec", "--json", "prompt", "--ignore-user-config"]);
    let error = translate(&command)
        .err()
        .expect("isolation must be preserved");
    assert!(
        error
            .to_string()
            .contains("cannot preserve --ignore-user-config")
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
    assert_eq!(launch.thread["approvalPolicy"], "never");
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
