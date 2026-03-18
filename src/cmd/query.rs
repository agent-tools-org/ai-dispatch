// Fast LLM query via OpenRouter API — no agent subprocess startup.
// Exports: run.
// Deps: serde_json, config, store.

use anyhow::{Context, Result};
use std::io::Write;
use std::process::Command;
use std::time::Instant;

const OPENROUTER_URL: &str = "https://openrouter.ai/api/v1/chat/completions";

pub fn run(
    store: &crate::store::Store,
    prompt: &str,
    model: Option<&str>,
    auto: bool,
    group: Option<&str>,
    finding: bool,
) -> Result<()> {
    let config = crate::config::load_config().unwrap_or_default();
    let qc = &config.query;

    let api_key = qc.api_key.clone()
        .or_else(|| std::env::var("OPENROUTER_API_KEY").ok())
        .or_else(|| std::env::var("AID_QUERY_KEY").ok())
        .ok_or_else(|| anyhow::anyhow!(
            "Set OPENROUTER_API_KEY or [query].api_key in ~/.aid/config.toml"
        ))?;

    let model = if let Some(m) = model {
        m.to_string()
    } else if auto {
        qc.auto_model.clone()
    } else {
        qc.free_model.clone()
    };

    let (content, cost, elapsed) = call_api(&api_key, &model, prompt)
        .map_err(|e| {
            if e.to_string().contains("429") {
                anyhow::anyhow!("{model} rate-limited. Try: aid query --auto \"...\"")
            } else {
                e
            }
        })?;

    aid_info!("[query] {model} {:.1}s (${:.6})", elapsed, cost);
    println!("{content}");

    if finding
        && let Some(gid) = group
    {
        store.insert_finding(gid, &content, None, None, None, None, None, None, None)?;
        aid_info!("[query] Finding saved to {gid}");
    }

    std::io::stdout().flush()?;
    Ok(())
}

fn call_api(api_key: &str, model: &str, prompt: &str) -> Result<(String, f64, f64)> {
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}]
    });
    let body_str = serde_json::to_string(&body)
        .context("serialize OpenRouter request body")?;

    let start = Instant::now();
    let output = Command::new("curl")
        .args([
            "-s",
            "-X",
            "POST",
            OPENROUTER_URL,
            "-H",
            &format!("Authorization: Bearer {api_key}"),
            "-H",
            "Content-Type: application/json",
            "-d",
            &body_str,
        ])
        .output()
        .context("run curl request")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow::anyhow!(
            "curl failed (status {}): {}",
            output.status,
            stderr
        ));
    }

    let elapsed = start.elapsed().as_secs_f64();
    let stdout = String::from_utf8(output.stdout)
        .context("read OpenRouter response from curl")?;
    let resp: serde_json::Value = serde_json::from_str(&stdout)
        .context("parse OpenRouter response JSON")?;

    let content = resp["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("(no response)")
        .to_string();
    let cost = resp["usage"]["cost"].as_f64().unwrap_or(0.0);

    Ok((content, cost, elapsed))
}
