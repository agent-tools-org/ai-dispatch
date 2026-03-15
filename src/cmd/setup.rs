// Interactive setup wizard for aid configuration.
// Exports: run.
// Deps: paths, config, std::io.

use anyhow::Result;
use std::io::{self, BufRead, Write};

pub fn run() -> Result<()> {
    println!();
    println!("  aid setup");
    println!("  ─────────────────────────────────");
    println!("  Press Enter to skip any step.");
    println!();

    let config_path = crate::paths::config_path();
    let mut existing = if config_path.exists() {
        std::fs::read_to_string(&config_path)?
    } else {
        String::new()
    };
    let config = crate::config::load_config().unwrap_or_default();
    let qc = &config.query;
    let current_env_key = std::env::var("OPENROUTER_API_KEY").ok();
    let has_key = qc.api_key.as_deref().filter(|k| !k.is_empty()).is_some()
        || current_env_key.is_some();

    // 1. OpenRouter API key
    section("OpenRouter API Key");
    println!("  Free model:  {}", qc.free_model);
    println!("  Auto model:  {}", qc.auto_model);

    if let Some(k) = qc.api_key.as_deref().filter(|k| !k.is_empty()) {
        println!("  API key:     {} (config.toml)", mask_key(k));
        print!("\n  Update key? Enter new key or press Enter to keep: ");
        io::stdout().flush()?;
        let key = read_line()?;
        if !key.is_empty() {
            eprint!("  Verifying... ");
            io::stderr().flush()?;
            if test_openrouter_key(&key) {
                eprintln!("OK");
                println!("  ✓ Key updated ({})", mask_key(&key));
                replace_api_key(&mut existing, &key);
            } else {
                eprintln!("failed");
                println!("  ✗ Could not verify — saved anyway");
                replace_api_key(&mut existing, &key);
            }
        }
    } else if let Some(k) = &current_env_key {
        println!("  API key:     {} (env)", mask_key(k));
        print!("\n  Save to config? Enter key or press Enter to keep using env: ");
        io::stdout().flush()?;
        let key = read_line()?;
        if !key.is_empty() {
            eprint!("  Verifying... ");
            io::stderr().flush()?;
            if test_openrouter_key(&key) {
                eprintln!("OK");
                println!("  ✓ Key saved ({})", mask_key(&key));
            } else {
                eprintln!("failed");
                println!("  ✗ Could not verify — saved anyway");
            }
            append_query_section(&mut existing, &key);
        }
    } else {
        println!("  API key:     not set");
        println!("\n  Enables `aid query` — fast LLM queries without agent startup.");
        println!("  Get a key at: https://openrouter.ai/keys\n");
        print!("  Key: ");
        io::stdout().flush()?;
        let key = read_line()?;
        if !key.is_empty() {
            eprint!("  Verifying... ");
            io::stderr().flush()?;
            if test_openrouter_key(&key) {
                eprintln!("OK");
                println!("  ✓ Key verified ({})", mask_key(&key));
            } else {
                eprintln!("failed");
                println!("  ✗ Could not verify — saved anyway");
            }
            append_query_section(&mut existing, &key);
        } else {
            println!("  Skipped");
        }
    }

    // 2. Detect installed agents
    section("Agents");
    let builtin = [
        ("gemini", "gemini"),
        ("codex", "codex"),
        ("opencode", "opencode"),
        ("cursor", "cursor"),
        ("kilo", "kilo"),
        ("codebuff", "codebuff"),
    ];
    let mut installed = 0;
    let mut missing = Vec::new();
    for (name, cmd) in builtin {
        let found = std::process::Command::new("which")
            .arg(cmd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        if found {
            installed += 1;
            println!("  ✓ {name}");
        } else {
            missing.push(name);
        }
    }
    let agents_dir = crate::paths::aid_dir().join("agents");
    if agents_dir.is_dir()
        && let Ok(entries) = std::fs::read_dir(&agents_dir)
    {
        for entry in entries.flatten() {
            if entry.path().extension().is_some_and(|e| e == "toml") {
                let name = entry.path().file_stem().unwrap().to_string_lossy().to_string();
                installed += 1;
                println!("  ✓ {name} (custom)");
            }
        }
    }
    if !missing.is_empty() {
        println!("  · not found: {}", missing.join(", "));
    }
    println!("  {installed} agent(s) ready");

    // 3. Write config
    let dir = config_path.parent().unwrap();
    std::fs::create_dir_all(dir)?;
    std::fs::write(&config_path, &existing)?;

    // 4. Summary — different for first-time vs returning
    if has_key {
        section("Status");
        println!("  Config: {}", config_path.display());
        println!("  Ready to use.");
    } else {
        section("Done");
        println!("  Config: {}", config_path.display());
        println!();
        println!("  Quick start:");
        println!("    aid query \"your question\"          free LLM query");
        println!("    aid query --auto \"question\"         paid, better quality");
        println!("    aid run codex \"task\" --worktree x   dispatch agent");
        println!("    aid init                            install skills & templates");
    }
    println!();
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
    ureq::post("https://openrouter.ai/api/v1/chat/completions")
        .header("Authorization", &format!("Bearer {key}"))
        .send_json(&body)
        .is_ok()
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
