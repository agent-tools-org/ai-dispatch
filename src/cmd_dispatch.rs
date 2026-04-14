// aid CLI command dispatch support.
// Exports shared dispatch helpers and finding-content resolution logic.

mod dispatch_match;
mod handlers_a;
mod handlers_b;
mod handlers_c;

use crate::cli::Commands;
use anyhow::{Result, bail};
use std::fs;
use std::io::{IsTerminal, Read};
use std::sync::Arc;

pub(crate) async fn dispatch(store: Arc<crate::store::Store>, command: Commands) -> Result<()> {
    dispatch_match::dispatch(store, command).await
}

pub(crate) fn resolve_group(flag: Option<String>) -> Option<String> {
    flag.or_else(|| std::env::var("AID_GROUP").ok())
}

pub(crate) fn resolve_finding_content(
    content: Option<String>,
    stdin: bool,
    file: Option<String>,
) -> Result<String> {
    let stdin_is_terminal = std::io::stdin().is_terminal();
    resolve_finding_content_from(content, stdin, file, stdin_is_terminal, &mut std::io::stdin())
}

pub(crate) fn resolve_finding_content_from<R: Read>(
    content: Option<String>,
    stdin: bool,
    file: Option<String>,
    _stdin_is_terminal: bool,
    reader: &mut R,
) -> Result<String> {
    if let Some(path) = file {
        return Ok(fs::read_to_string(path)?);
    }
    // Only read stdin when --stdin is explicitly passed (#101).
    // Previously this also auto-read when stdin was not a terminal,
    // but in background tasks stdin is /dev/null, causing empty reads.
    if stdin {
        let mut buffer = String::new();
        reader.read_to_string(&mut buffer)?;
        return Ok(buffer);
    }
    if let Some(content) = content {
        return Ok(content);
    }
    bail!("No finding content provided. Pass content as an argument, --file <path>, or --stdin")
}

#[cfg(test)]
mod tests {
    use super::resolve_finding_content_from;
    use anyhow::Result;
    use std::io::{Cursor, Write};
    use tempfile::NamedTempFile;

    #[test]
    fn resolve_finding_content_prefers_file() -> Result<()> {
        let mut file = NamedTempFile::new()?;
        write!(file, "from file")?;
        let mut stdin = Cursor::new("from stdin");

        let content = resolve_finding_content_from(
            Some("inline".to_string()),
            true,
            Some(file.path().to_string_lossy().into_owned()),
            false,
            &mut stdin,
        )?;

        assert_eq!(content, "from file");
        Ok(())
    }

    #[test]
    fn resolve_finding_content_reads_stdin_when_requested() -> Result<()> {
        let mut stdin = Cursor::new("from stdin");
        let content = resolve_finding_content_from(
            Some("inline".to_string()),
            true,
            None,
            true,
            &mut stdin,
        )?;
        assert_eq!(content, "from stdin");
        Ok(())
    }

    #[test]
    fn resolve_finding_content_errors_when_piped_without_stdin_flag() {
        let mut stdin = Cursor::new("from pipe");
        let err = resolve_finding_content_from(None, false, None, false, &mut stdin).unwrap_err();
        assert!(err.to_string().contains("No finding content provided"));
    }

    #[test]
    fn resolve_finding_content_uses_inline_arg() -> Result<()> {
        let mut stdin = Cursor::new("");
        let content = resolve_finding_content_from(
            Some("inline".to_string()),
            false,
            None,
            true,
            &mut stdin,
        )?;
        assert_eq!(content, "inline");
        Ok(())
    }

    #[test]
    fn resolve_finding_content_errors_without_input() {
        let mut stdin = Cursor::new("");
        let error = resolve_finding_content_from(None, false, None, true, &mut stdin).unwrap_err();
        assert!(error.to_string().contains("No finding content provided"));
    }
}
