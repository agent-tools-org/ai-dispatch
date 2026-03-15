// Interactive setup wizard for aid configuration.
// Exports: run.
// Deps: paths, config, std::io.

use anyhow::Result;
use std::io::{self, BufRead, Write};

pub fn run() -> Result<()> {
    println!("aid setup — interactive configuration");
    println!("Press Enter to skip any step.\n");

    let config_path = crate::paths::config_path();
    let mut existing = if config_path.exists() {
        std::fs::read_to_string(&config_path)?
    } else {
        String::new()
    };

    // 1. OpenRouter API key
    let current_key = std::env::var("OPENROUTER_API_KEY").ok();
    if existing.contains("api_key") {
        println!("[query] API key already configured in config.toml");
    } else if current_key.is_some() {
        println!("[query] Using OPENROUTER_API_KEY from environment");
    } else {
        print!("OpenRouter API key (get one at https://openrouter.ai/keys): ");
        io::stdout().flush()?;
        let key = read_line()?;
        if !key.is_empty() {
            if test_openrouter_key(&key) {
                println!("  ✓ Key verified");
                append_query_section(&mut existing, &key);
            } else {
                println!("  ✗ Key test failed — saving anyway, check it later");
                append_query_section(&mut existing, &key);
            }
        } else {
            println!("  Skipped (set OPENROUTER_API_KEY env var later)");
        }
    }

    // 2. Detect installed agents
    println!("\nDetecting installed agents...");
    let agents = [
        ("gemini", "gemini"),
        ("codex", "codex"),
        ("opencode", "opencode"),
        ("cursor", "cursor"),
        ("kilo", "kilo"),
    ];
    for (name, cmd) in agents {
        let found = std::process::Command::new("which")
            .arg(cmd)
            .output()
            .map(|o| o.status.success())
            .unwrap_or(false);
        println!("  {} {name}", if found { "✓" } else { "✗" });
    }

    // 3. Write config
    let dir = config_path.parent().unwrap();
    std::fs::create_dir_all(dir)?;
    std::fs::write(&config_path, &existing)?;
    println!("\nConfig saved to {}", config_path.display());

    // 4. Summary
    println!("\nQuick start:");
    println!("  aid query \"your question\"        # free LLM query");
    println!("  aid query --auto \"question\"       # paid, better quality");
    println!("  aid run codex \"task\" --worktree x  # dispatch agent");
    println!("  aid init                           # install skills & templates");
    Ok(())
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
