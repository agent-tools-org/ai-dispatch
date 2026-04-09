// Handler for `aid credential` subcommands.
// Exports: CredentialAction, run_credential_command.
// Deps: crate::credential_pool, chrono, anyhow.

use crate::credential_pool;
use anyhow::{Result, bail};
use chrono::Local;

pub enum CredentialAction {
    List,
    Add {
        provider: String,
        name: String,
        env: String,
    },
    Remove {
        provider: String,
        name: String,
    },
}

pub(crate) fn run_credential_command(action: CredentialAction) -> Result<()> {
    match action {
        CredentialAction::List => list_credentials(),
        CredentialAction::Add { provider, name, env } => {
            bail!("Not implemented yet: aid credential add {provider} {name} {env}")
        }
        CredentialAction::Remove { provider, name } => {
            bail!("Not implemented yet: aid credential remove {provider} {name}")
        }
    }
}

fn list_credentials() -> Result<()> {
    let Some(pool) = credential_pool::load_pool() else {
        println!(
            "No credential pool configured at {}",
            crate::paths::aid_dir().join("credentials.toml").display()
        );
        return Ok(());
    };
    print!("{}", render_credential_list(&pool));
    Ok(())
}

fn render_credential_list(pool: &credential_pool::CredentialPool) -> String {
    let mut providers: Vec<_> = pool.providers.iter().collect();
    providers.sort_by(|left, right| left.0.cmp(right.0));
    let now = Local::now();
    let mut lines = Vec::new();
    for (provider_name, provider) in providers {
        lines.push(format!("{provider_name} ({:?})", provider.strategy));
        for key in &provider.keys {
            let status = match key.exhausted_until.as_ref() {
                Some(until) if *until > now => format!("cooldown until {}", until.to_rfc3339()),
                _ => "ready".to_string(),
            };
            let env_status = if std::env::var(&key.env).ok().filter(|value| !value.is_empty()).is_some() {
                "set"
            } else {
                "missing"
            };
            lines.push(format!("  {} [{}] {} ({})", key.name, key.env, status, env_status));
        }
    }
    format!("{}\n", lines.join("\n"))
}

#[cfg(test)]
mod tests {
    use super::{CredentialAction, render_credential_list, run_credential_command};
    use crate::credential_pool::{load_pool, mark_exhausted};
    use crate::paths::AidHomeGuard;
    use std::fs;
    use tempfile::TempDir;

    const SAMPLE: &str = r#"
[codex]
strategy = "round_robin"
keys = [
  { name = "personal", env = "OPENAI_API_KEY" },
]
"#;

    #[test]
    fn run_credential_command_list_handles_missing_file() {
        let dir = TempDir::new().unwrap();
        let _guard = AidHomeGuard::set(dir.path());

        run_credential_command(CredentialAction::List).unwrap();
    }

    #[test]
    fn run_credential_command_add_is_stubbed() {
        let error = run_credential_command(CredentialAction::Add {
            provider: "codex".to_string(),
            name: "personal".to_string(),
            env: "OPENAI_API_KEY".to_string(),
        })
        .unwrap_err();

        assert!(error.to_string().contains("Not implemented yet"));
    }

    #[test]
    fn render_credential_list_shows_provider_status() {
        let dir = TempDir::new().unwrap();
        let _guard = AidHomeGuard::set(dir.path());
        fs::write(dir.path().join("credentials.toml"), SAMPLE).unwrap();
        unsafe {
            std::env::set_var("OPENAI_API_KEY", "secret");
        }

        let pool = load_pool().unwrap();

        let rendered = render_credential_list(&pool);

        assert!(rendered.contains("codex (RoundRobin)"));
        assert!(rendered.contains("personal [OPENAI_API_KEY] ready (set)"));
    }

    #[test]
    fn render_credential_list_shows_cooldown_status() {
        let dir = TempDir::new().unwrap();
        let _guard = AidHomeGuard::set(dir.path());
        fs::write(dir.path().join("credentials.toml"), SAMPLE).unwrap();
        let mut pool = load_pool().unwrap();

        mark_exhausted(&mut pool, "codex", "personal", 5);
        let rendered = render_credential_list(&pool);

        assert!(rendered.contains("cooldown until"));
    }
}
