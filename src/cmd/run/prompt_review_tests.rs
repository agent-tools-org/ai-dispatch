// Real prompt-bundle regression for advisory review from a context-rich project.
// A subprocess owns cwd so parallel tests never observe a changed working directory.
use super::*;
use std::{path::Path, process::Command};

#[test]
fn review_request_is_read_only_and_contains_no_project_context() {
    if std::env::var_os("AID_REVIEW_FIXTURE_CHILD").is_some() {
        check_isolation();
        return;
    }
    let repo = tempfile::tempdir().expect("repo");
    assert!(
        Command::new("git")
            .args(["init", "-q"])
            .arg(repo.path())
            .status()
            .expect("git")
            .success()
    );
    std::fs::create_dir(repo.path().join(".aid")).expect("config directory");
    std::fs::write(
        repo.path().join(".aid/project.toml"),
        concat!(
            "[project]\nid = 'review-fixture'\nteam = 'review-team'\n",
            "rules = ['DISTINCT_PROJECT_RULE']\nskills = ['review-skill']\n",
            "verify = 'false'\nsetup = 'false'\nremote_build = 'auto'\n",
        ),
    )
    .expect("project config");
    let test_module = module_path!().split_once("::").expect("crate prefix").1;
    let output = Command::new(std::env::current_exe().expect("test binary"))
        .args([
            "--exact",
            &format!(
                "{}::review_request_is_read_only_and_contains_no_project_context",
                test_module
            ),
            "--nocapture",
        ])
        .env("AID_REVIEW_FIXTURE_CHILD", "1")
        .current_dir(repo.path())
        .output()
        .expect("isolated test child");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(output.status.success(), "{stdout}");
    assert!(stdout.contains("1 passed; 0 failed"), "{stdout}");
}

fn seed_context(store: &Store, root: &Path) {
    let home = crate::support::paths::aid_dir();
    for dir in ["skills", "teams", "tools"] {
        std::fs::create_dir_all(home.join(dir)).expect("context directory");
    }
    std::fs::write(
        home.join("skills/review-skill.md"),
        "DISTINCT_DEFAULT_SKILL",
    )
    .expect("skill");
    std::fs::write(
        home.join("teams/review-team.toml"),
        concat!(
            "[team]\nid = 'review-team'\ndisplay_name = 'Review team'\n",
            "preferred_agents = []\nrules = ['DISTINCT_TEAM_RULE']\n",
        ),
    )
    .expect("team");
    std::fs::write(
        home.join("tools/review.toml"),
        concat!(
            "[tool]\nid = 'review-tool'\ncommand = 'false'\n",
            "description = 'DISTINCT_GLOBAL_TOOL'\n",
        ),
    )
    .expect("tool");
    store
        .db()
        .execute(
            "INSERT INTO memories
        (id,memory_type,content,project_path,content_hash,created_at)
        VALUES ('m-review','fact','DISTINCT_MEMORY',?1,'review-memory',?2)",
            rusqlite::params![root.to_string_lossy(), chrono::Local::now().to_rfc3339()],
        )
        .expect("memory");
}

fn check_isolation() {
    let home = tempfile::tempdir().expect("home");
    let _guard = crate::support::paths::AidHomeGuard::set(home.path());
    let caller = std::env::current_dir().expect("project cwd");
    let (_, project) = project::resolve_project_in(&caller);
    let project = project.expect("project fixture detected");
    assert_eq!(project.team.as_deref(), Some("review-team"));
    assert_eq!(project.skills, vec!["review-skill".to_string()]);
    let store = Store::open_memory().expect("store");
    seed_context(&store, &caller);
    assert!(team::resolve_team("review-team").is_some());
    assert_eq!(toolbox::list_tools().len(), 1);
    let neutral = tempfile::tempdir().expect("neutral directory");
    assert_eq!(
        std::fs::read_dir(neutral.path())
            .expect("empty directory")
            .count(),
        0
    );
    let args = crate::cmd::ask::review::review_args(
        "REVIEW_MARKER_ONLY".into(),
        "agy".into(),
        neutral.path(),
        &neutral.path().join("answer.txt"),
    );
    assert!(args.read_only && args.no_audit && args.no_backup);
    assert!(args.team.is_none() && args.verify.is_none() && args.setup.is_none());
    assert!(args.remote_build.is_none() && !args.audit);
    assert!(!Path::new(args.dir.as_deref().expect("dir")).starts_with(&caller));
    let (root, detected) =
        project::resolve_project_in(Path::new(args.dir.as_deref().expect("dir")));
    assert!(root.is_none() && detected.is_none());
    check_bundle(&store, &args, detected.as_ref(), root.as_deref());
}

fn check_bundle(
    store: &Store,
    args: &RunArgs,
    detected: Option<&project::ProjectConfig>,
    root: Option<&Path>,
) {
    let skills = effective_skills(args, detected);
    assert!(skills.is_empty());
    let bundle = build_prompt_bundle(
        store,
        args,
        &AgentKind::Antigravity,
        None,
        &skills,
        "t-review",
        detected,
        root,
    )
    .expect("prompt bundle");
    for forbidden in [
        "DISTINCT_",
        "[Memory]",
        "Project Rules",
        "Team Rules",
        "Toolbox",
        "[MILESTONE]",
        "git add",
        "implementer",
    ] {
        assert!(!bundle.effective_prompt.contains(forbidden), "{forbidden}");
    }
    assert!(bundle.effective_prompt.starts_with("REVIEW_MARKER_ONLY"));
    assert!(bundle.context_files.is_empty() && bundle.injected_memory_ids.is_empty());
}
