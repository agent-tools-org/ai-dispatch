// Large initial prompts must survive slow Claude startup without partial delivery.
// Uses the production interactive-spec route and an unpaid streaming CLI fixture.
use super::*;

#[tokio::test]
async fn large_prompt_arrives_intact_after_more_than_five_seconds_of_startup() {
    let _permit = crate::support::test_subprocess::acquire();
    let (home, _guard, store, spec, args) = setup();
    let output = home.path().join("initial.json");
    let prompt = "x".repeat(256 * 1024);
    let original = gated_tests::gated_command("delayed", &output);
    let mut launch = std::process::Command::new(original.get_program());
    for arg in original.get_args() {
        launch.arg(if arg == "initial prompt" {
            prompt.as_ref()
        } else {
            arg
        });
    }
    let observer = async {
        wait_for_file(&output.with_extension("ready")).await;
        // This interval is the behavior under test: initial writes have no five-second limit.
        let held = std::time::Instant::now();
        tokio::time::sleep(Duration::from_millis(5500)).await;
        assert!(held.elapsed() > Duration::from_secs(5));
        std::fs::write(output.with_extension("release"), "go").unwrap();
    };
    let run = run_process(
        &store,
        &spec,
        &args,
        &ClaudeAgent,
        launch,
        std::time::Instant::now(),
    );
    let (result, ()) = tokio::time::timeout(Duration::from_secs(120), async {
        tokio::join!(run, observer)
    })
    .await
    .unwrap();
    result.unwrap();
    assert_eq!(status(&store), TaskStatus::Done);
    let initial: serde_json::Value =
        serde_json::from_slice(&std::fs::read(output).unwrap()).unwrap();
    assert_eq!(initial["message"]["content"], prompt);
    assert_eq!(initial["uuid"], "00000000-0000-4000-8000-000000000000");
}
