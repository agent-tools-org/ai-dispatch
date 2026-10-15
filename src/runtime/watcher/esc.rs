// Terminal escape stripping shared by watcher stream and PTY monitor.
// Exports strip_terminal_escapes: removes OSC and CSI sequences so
// OSC-prefixed agent stream-json lines (e.g. droid >=0.159 under a PTY)
// still parse as events.

use std::borrow::Cow;

/// Strip terminal escape sequences from agent output.
///
/// Removes OSC (`ESC ]` ... terminated by BEL or ST `ESC \`) and CSI
/// (`ESC [` params/intermediates plus one final byte). An OSC left
/// unterminated is dropped through end-of-line; a newline acts as an
/// implicit terminator so multi-line input is never swallowed.
pub(crate) fn strip_terminal_escapes(input: &str) -> Cow<'_, str> {
    if !input.contains('\x1b') {
        return Cow::Borrowed(input);
    }
    let bytes = input.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            i = match bytes.get(i + 1) {
                Some(b']') => skip_osc(bytes, i + 2),
                Some(b'[') => skip_csi(bytes, i + 2),
                // Lone or unknown escape: drop the ESC byte itself.
                _ => i + 1,
            };
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    // Escape sequences are pure ASCII, so removing them keeps valid UTF-8.
    Cow::Owned(String::from_utf8_lossy(&out).into_owned())
}

/// Skip past an OSC body starting at `from` (after `ESC ]`).
/// Returns the index of the first byte after the sequence.
fn skip_osc(bytes: &[u8], from: usize) -> usize {
    let mut j = from;
    while j < bytes.len() {
        match bytes[j] {
            0x07 => return j + 1,
            b'\n' => return j,
            0x1b if bytes.get(j + 1) == Some(&b'\\') => return j + 2,
            _ => j += 1,
        }
    }
    bytes.len()
}

/// Skip past a CSI body starting at `from` (after `ESC [`).
/// Returns the index of the first byte after the sequence.
fn skip_csi(bytes: &[u8], from: usize) -> usize {
    let mut j = from;
    while j < bytes.len() && (0x20..=0x3f).contains(&bytes[j]) {
        j += 1;
    }
    if j < bytes.len() && (0x40..=0x7e).contains(&bytes[j]) {
        j + 1
    } else {
        j
    }
}

#[cfg(test)]
mod tests {
    use super::strip_terminal_escapes;

    #[test]
    fn plain_lines_pass_through_borrowed() {
        let line = r#"{"type":"message","text":"hi"}"#;
        assert!(matches!(
            strip_terminal_escapes(line),
            std::borrow::Cow::Borrowed(_)
        ));
        assert_eq!(strip_terminal_escapes(line), line);
    }

    #[test]
    fn real_droid_osc_prefixed_lines_parse_after_stripping() {
        let samples = [
            (
                "\x1b]0;⛬ reply pong\x07{\"type\":\"system\",\"subtype\":\"init\"}",
                "{\"type\":\"system\",\"subtype\":\"init\"}",
            ),
            (
                "\x1b]9;4;0;\x07\x1b]9;4;3;\x07{\"type\":\"message\",\"role\":\"user\",\"text\":\"hi\"}",
                "{\"type\":\"message\",\"role\":\"user\",\"text\":\"hi\"}",
            ),
            (
                "\x1b]9;4;0;\x07{\"type\":\"completion\",\"finalText\":\"pong\"}",
                "{\"type\":\"completion\",\"finalText\":\"pong\"}",
            ),
        ];
        for (raw, expected) in samples {
            let stripped = strip_terminal_escapes(raw);
            assert_eq!(stripped, expected);
            assert!(serde_json::from_str::<serde_json::Value>(&stripped).is_ok());
        }
    }

    #[test]
    fn osc_terminated_by_st_is_stripped() {
        let line = "\x1b]0;title\x1b\\{\"type\":\"text\",\"text\":\"ok\"}";
        assert_eq!(
            strip_terminal_escapes(line),
            "{\"type\":\"text\",\"text\":\"ok\"}"
        );
    }

    #[test]
    fn csi_sequences_still_stripped() {
        let line = "\x1b[1m\x1b[32mHello\x1b[0m \x1b[38;5;202mWorld\x1b[0m";
        assert_eq!(strip_terminal_escapes(line), "Hello World");
        assert_eq!(strip_terminal_escapes("\x1b[?25lspinner\x1b[?25h"), "spinner");
    }

    #[test]
    fn multibyte_text_outside_escapes_is_preserved() {
        let line = "\x1b[1m⛬ 完成\x1b[0m";
        assert_eq!(strip_terminal_escapes(line), "⛬ 完成");
    }

    #[test]
    fn unterminated_osc_at_eol_does_not_panic_or_loop() {
        assert_eq!(strip_terminal_escapes("\x1b]0;half-title"), "");
        assert_eq!(strip_terminal_escapes("data\x1b]9;4;1"), "data");
        assert_eq!(strip_terminal_escapes("\x1b]"), "");
        assert_eq!(strip_terminal_escapes("\x1b"), "");
    }

    #[test]
    fn newline_terminates_osc_in_multiline_input() {
        let text = "\x1b]0;title\nnext line";
        assert_eq!(strip_terminal_escapes(text), "\nnext line");
    }
}
