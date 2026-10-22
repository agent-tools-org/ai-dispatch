// Interactive setup wizard for aid configuration.
// Exports: run.
// Deps: paths, config, std::io.

use anyhow::{Context, Result};
use std::io::{self, BufRead, Write};
use std::process::Command;

#[path = "setup_typesafe.rs"]
mod typesafe;

mod openrouter;

pub fn run() -> Result<()> {
    println!();
    println!("  aid setup");
    println!("  ─────────────────────────────────");
    println!("  Press Enter to skip any step.");
    println!();

    let config_path = crate::support::paths::config_path();
    let mut existing = if config_path.exists() {
        std::fs::read_to_string(&config_path)?
    } else {
        String::new()
    };
    let config = crate::config::load_config().unwrap_or_default();
    let qc = &config.query;
    let current_env_key = std::env::var("OPENROUTER_API_KEY").ok();
    let has_key = qc.api_key.as_deref().filter(|k| !k.is_empty()).is_some() || current_env_key.is_some();

    // 1. OpenRouter API key
    openrouter::configure(qc, current_env_key.as_deref(), &mut existing)?;

    section("TypeSafe (Jev)");
    typesafe::run()?;

    // 2. Detect installed agents
    detect_installed_agents();

    // 3. Write config
    let dir = config_path.parent().context("config path has no parent")?;
    std::fs::create_dir_all(dir)?;
    std::fs::write(&config_path, &existing)?;

    initialize_skills()?;

    print_setup_summary(has_key, &config_path);
    Ok(())
}

fn section(title: &str) {
    println!();
    println!("  [{title}]");
}

fn mask_key(key: &str) -> String {
    if key.len() > 12 {
        format!("{}...{}", &key[..8], &key[key.len() - 4..])
    } else {
        "****".to_string()
    }
}

fn read_line() -> Result<String> {
    let mut line = String::new();
    io::stdin().lock().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

fn test_openrouter_key(key: &str) -> bool {
    let body = serde_json::json!({
        "model": "openrouter/free",
        "messages": [{"role": "user", "content": "ping"}],
        "max_tokens": 1
    });
    let body_str = match serde_json::to_string(&body) {
        Ok(s) => s,
        Err(_) => return false,
    };

    let output = match Command::new("curl")
        .args([
            "-s",
            "-X",
            "POST",
            "https://openrouter.ai/api/v1/chat/completions",
            "-H",
            &format!("Authorization: Bearer {key}"),
            "-H",
            "Content-Type: application/json",
            "-d",
            &body_str,
        ])
        .output()
    {
        Ok(out) => out,
        Err(_) => return false,
    };

    if !output.status.success() {
        return false;
    }

    let stdout = String::from_utf8_lossy(&output.stdout).to_lowercase();
    !stdout.contains("error")
}

fn append_query_section(config: &mut String, key: &str) {
    if !config.contains("[query]") {
        if !config.is_empty() && !config.ends_with('\n') {
            config.push('\n');
        }
        config.push_str(&format!("\n[query]\napi_key = \"{key}\"\n"));
    }
}

fn replace_api_key(config: &mut String, new_key: &str) {
    if let Some(start) = config.find("api_key")
        && let Some(eq) = config[start..].find('=')
    {
        let val_start = start + eq + 1;
        let line_end = config[val_start..]
            .find('\n')
            .map(|p| val_start + p)
            .unwrap_or(config.len());
        config.replace_range(val_start..line_end, &format!(" \"{new_key}\""));
    }
}

fn detect_installed_agents() {
    // 2. Detect installed agents
    section("Agents");
    let builtin = [
        ("agy", crate::types::AgentKind::Antigravity),
        ("qwen", crate::types::AgentKind::Qwen),
        ("codex", crate::types::AgentKind::Codex),
        ("copilot", crate::types::AgentKind::Copilot),
        ("opencode", crate::types::AgentKind::OpenCode),
        ("cursor", crate::types::AgentKind::Cursor),
        ("kilo", crate::types::AgentKind::Kilo),
        ("mimocode", crate::types::AgentKind::MiMoCode),
        ("droid", crate::types::AgentKind::Droid),
        ("oz", crate::types::AgentKind::Oz),
        ("claude", crate::types::AgentKind::Claude),
    ];
    let detected = crate::agent::detect_agents();
    let mut installed = 0;
    let mut missing = Vec::new();
    for (name, kind) in builtin {
        let found = detected.contains(&kind);
        if found {
            installed += 1;
            println!("  ✓ {name}");
        } else {
            missing.push(name);
        }
    }
    print_custom_agents(&mut installed);
    if !missing.is_empty() {
        println!("  · not found: {}", missing.join(", "));
    }
    println!("  {installed} agent(s) ready");
}

fn print_custom_agents(installed: &mut usize) {
    let agents_dir = crate::support::paths::aid_dir().join("agents");
    if agents_dir.is_dir()
        && let Ok(entries) = std::fs::read_dir(&agents_dir)
    {
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|e| e == "toml") {
                let path = entry.path();
                let Some(stem) = path.file_stem() else { continue };
                let name = stem.to_string_lossy().to_string();
                *installed += 1;
                println!("  ✓ {name} (custom)");
            }
        }
    }
}

fn initialize_skills() -> Result<()> {
    // 4. Auto-init skills & templates if missing
    let skills_dir = crate::support::paths::aid_dir().join("skills");
    let needs_init = !skills_dir.is_dir()
        || std::fs::read_dir(&skills_dir)
            .map(|d| d.count() == 0)
            .unwrap_or(true);
    if needs_init {
        section("Skills & Templates");
        crate::cmd::init::run()?;
    } else {
        crate::cmd::init::refresh_official_guide()?;
    }

    Ok(())
}

fn print_setup_summary(has_key: bool, config_path: &std::path::Path) {
    // 5. Summary
    section(if has_key { "Status" } else { "Done" });
    println!("  Config: {}", config_path.display());
    if has_key {
        println!("  Ready to use.");
    } else {
        println!();
        println!("  Quick start:");
        println!("    aid query \"your question\"          free LLM query");
        println!("    aid query --auto \"question\"         paid, better quality");
        println!("    aid run codex \"task\" --worktree x   dispatch agent");
    }
    println!();
}
