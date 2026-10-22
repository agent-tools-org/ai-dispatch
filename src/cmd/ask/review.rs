// Isolated, read-only one-shot review using the ordinary task runner.
// Builds review RunArgs without project defaults or automatically attached context.
use super::*;

pub(crate) async fn ask_review_text(
    store: Arc<Store>,
    prompt: String,
    agent: Option<String>,
) -> Result<String> {
    let directory = ReviewDirectory::create()?;
    let capture = directory.0.join("answer.txt");
    let request =
        prepare_request_with_context(prompt, agent, None, vec![], None, ContextFiles::Disabled)?;
    let args = review_args(request.prompt, request.agent_name, &directory.0, &capture);
    let task = run::run(store, args).await?;
    read_answer(&task, &capture)
}

pub(crate) fn review_args(prompt: String, agent: String, dir: &Path, output: &Path) -> RunArgs {
    RunArgs {
        prompt,
        agent_name: agent,
        dir: Some(dir.to_string_lossy().into_owned()),
        output: Some(output.to_string_lossy().into_owned()),
        read_only: true,
        skills: vec![run::NO_SKILL_SENTINEL.into()],
        no_audit: true,
        no_backup: true,
        announce: false,
        ..Default::default()
    }
}

struct ReviewDirectory(PathBuf);
impl ReviewDirectory {
    fn create() -> Result<Self> {
        let path = crate::agent::scratch::create_temp_dir(&std::env::temp_dir())?;
        let directory = Self(path);
        anyhow::ensure!(
            crate::config::project::resolve_project_in(&directory.0)
                .0
                .is_none(),
            "Review temporary directory must be outside a Git project"
        );
        Ok(directory)
    }
}
impl Drop for ReviewDirectory {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(self.0.join("answer.txt"));
        let _ = std::fs::remove_dir(&self.0);
    }
}
