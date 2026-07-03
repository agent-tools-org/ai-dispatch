// Strip terminal escape sequences (OSC and CSI) from watcher input lines.
// Shared helper used by watcher stream and PTY watchers.

pub(crate) fn strip_terminal_escapes(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == 0x1b {
            if i + 1 < bytes.len() {
                let next = bytes[i + 1];
                // OSC: ESC ] ... terminated by BEL (0x07) or ST (ESC \\)
                if next == b']' {
                    let mut j = i + 2;
                    let mut terminated = false;
                    while j < bytes.len() {
                        if bytes[j] == 0x07 {
                            j += 1;
                            terminated = true;
                            break;
                        }
                        if bytes[j] == 0x1b && j + 1 < bytes.len() && bytes[j + 1] == b'\\' {
                            j += 2;
                            terminated = true;
                            break;
                        }
                        // Newline acts as implicit terminator for OSC; return at newline
                        if bytes[j] == b'\n' {
                            terminated = true;
                            j += 1;
                            break;
                        }
                        j += 1;
                    }
                    if terminated {
                        i = j;
                        continue;
                    } else {
                        // Unterminated OSC: skip the introducer and continue
                        i += 2;
                        continue;
                    }
                }
                // CSI: ESC [ ... final byte in 0x40..=0x7E
                if next == b'[' {
                    let mut j = i + 2;
                    while j < bytes.len() {
                        let b = bytes[j];
                        if (0x40..=0x7E).contains(&b) {
                            j += 1;
                            break;
                        }
                        j += 1;
                    }
                    i = j;
                    continue;
                }
                // Unknown two-byte ESC sequence: skip both
                i += 2;
                continue;
            }
            // Lone ESC: skip
            i += 1;
            continue;
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}
