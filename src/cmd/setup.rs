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

    // 1. OpenRouter API key
    section("OpenRouter API Key");
    let current_key = std::env::var("OPENROUTER_API_KEY").ok();
    if existing.contains("api_key") {
        println!("  Already configured in config.toml");
    } else if let Some(k) = &current_key {
        println!("  Using OPENROUTER_API_KEY from environment");
        println!("  {}", mask_key(k));
    } else {
        println!("  Enables `aid query` — fast LLM queries without agent startup.");
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
                append_query_section(&mut existing, &key);
            } else {
                eprintln!("failed");
                println!("  ✗ Could not verify — saved anyway, check it later");
                append_query_section(&mut existing, &key);
            }
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
        ("ob1", "ob1"),
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
    // Custom agents from ~/.aid/agents/
    let agents_dir = crate::paths::aid_dir().join("agents");
    if agents_dir.is_dir() {
        if let Ok(entries) = std::fs::read_dir(&agents_dir) {
            for entry in entries.flatten() {
                if entry.path().extension().is_some_and(|e| e == "toml") {
                    let name = entry.path().file_stem().unwrap().to_string_lossy().to_string();
                    installed += 1;
                    println!("  ✓ {name} (custom)");
                }
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

    // 4. Summary
    section("Done");
    println!("  Config: {}", config_path.display());
    println!();
    println!("  Quick start:");
    println!("    aid query \"your question\"          free LLM query");
    println!("    aid query --auto \"question\"         paid, better quality");
    println!("    aid run codex \"task\" --worktree x   dispatch agent");
    println!("    aid init                            install skills & templates");
    println!();
    Ok(())
}

fn section(title: &str) {
    println!();
    println!("  [{title}]");
}

fn mask_key(key: &str) -> String {
    if key.len() > 12 {
        format!("{}...{}", &key[..8], &key[key.len()-4..])
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
