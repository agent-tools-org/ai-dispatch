// Fast LLM query via OpenRouter API — no agent subprocess startup.
// Exports: run.
// Deps: ureq, serde_json, config, store.

use anyhow::Result;
use std::io::Write;
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

    eprintln!("[query] {model} {:.1}s (${:.6})", elapsed, cost);
    println!("{content}");

    if finding {
        if let Some(gid) = group {
            store.insert_finding(gid, &content, None)?;
            eprintln!("[query] Finding saved to {gid}");
        }
    }

    std::io::stdout().flush()?;
    Ok(())
}

fn call_api(api_key: &str, model: &str, prompt: &str) -> Result<(String, f64, f64)> {
    let body = serde_json::json!({
        "model": model,
        "messages": [{"role": "user", "content": prompt}]
    });

    let start = Instant::now();
    let resp: serde_json::Value = ureq::post(OPENROUTER_URL)
        .header("Authorization", &format!("Bearer {api_key}"))
        .send_json(&body)?
        .into_body()
        .read_json()?;

    let elapsed = start.elapsed().as_secs_f64();
    let content = resp["choices"][0]["message"]["content"]
        .as_str()
        .unwrap_or("(no response)")
        .to_string();
    let cost = resp["usage"]["cost"].as_f64().unwrap_or(0.0);

    Ok((content, cost, elapsed))
}
