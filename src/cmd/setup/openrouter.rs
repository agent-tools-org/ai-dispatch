// OpenRouter setup prompts and configuration updates.
// Exports: configure for the setup wizard.
// Deps: parent input, key verification, and config editing helpers.

use super::*;

pub(super) fn configure(
    qc: &crate::config::QueryConfig,
    current_env_key: Option<&str>,
    existing: &mut String,
) -> Result<()> {
    // 1. OpenRouter API key
    section("OpenRouter API Key");
    println!("  Free model:  {}", qc.free_model);
    println!("  Auto model:  {}", qc.auto_model);

    if let Some(k) = qc.api_key.as_deref().filter(|k| !k.is_empty()) {
        update_key(k, existing)?;
    } else if let Some(k) = current_env_key {
        save_environment_key(k, existing)?;
    } else {
        enter_key(existing)?;
    }

    Ok(())
}

fn update_key(k: &str, existing: &mut String) -> Result<()> {
    println!("  API key:     {} (config.toml)", mask_key(k));
    print!("\n  Update key? Enter new key or press Enter to keep: ");
    io::stdout().flush()?;
    let key = read_line()?;
    if !key.is_empty() {
        eprint!("  Verifying... ");
        io::stderr().flush()?;
        if test_openrouter_key(&key) {
            aid_info!("OK");
            println!("  ✓ Key updated ({})", mask_key(&key));
            replace_api_key(existing, &key);
        } else {
            aid_warn!("failed");
            println!("  ✗ Could not verify — saved anyway");
            replace_api_key(existing, &key);
        }
    }
    Ok(())
}

fn save_environment_key(k: &str, existing: &mut String) -> Result<()> {
    println!("  API key:     {} (env)", mask_key(k));
    print!("\n  Save to config? Enter key or press Enter to keep using env: ");
    io::stdout().flush()?;
    let key = read_line()?;
    if !key.is_empty() {
        eprint!("  Verifying... ");
        io::stderr().flush()?;
        if test_openrouter_key(&key) {
            aid_info!("OK");
            println!("  ✓ Key saved ({})", mask_key(&key));
        } else {
            aid_warn!("failed");
            println!("  ✗ Could not verify — saved anyway");
        }
        append_query_section(existing, &key);
    }
    Ok(())
}

fn enter_key(existing: &mut String) -> Result<()> {
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
            aid_info!("OK");
            println!("  ✓ Key verified ({})", mask_key(&key));
        } else {
            aid_warn!("failed");
            println!("  ✗ Could not verify — saved anyway");
        }
        append_query_section(existing, &key);
    } else {
        println!("  Skipped");
    }
    Ok(())
}
