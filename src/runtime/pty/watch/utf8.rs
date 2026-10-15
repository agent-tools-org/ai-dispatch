// Incremental UTF-8 decoding for PTY byte streams.
// Exports Utf8Chunks to preserve split multibyte sequences across reads.
// Depends only on std string decoding.

#[derive(Default)]
pub(super) struct Utf8Chunks {
    pending: Vec<u8>,
}

impl Utf8Chunks {
    pub(super) fn push(&mut self, bytes: Vec<u8>) -> String {
        let mut buffer = std::mem::take(&mut self.pending);
        buffer.extend_from_slice(&bytes);
        decode_complete_prefix(&mut self.pending, buffer)
    }

    pub(super) fn flush(&mut self) -> String {
        String::from_utf8_lossy(&std::mem::take(&mut self.pending)).into_owned()
    }
}

fn decode_complete_prefix(pending: &mut Vec<u8>, mut buffer: Vec<u8>) -> String {
    match std::str::from_utf8(&buffer) {
        Ok(text) => text.to_string(),
        Err(err) if err.error_len().is_none() => {
            let valid_up_to = err.valid_up_to();
            *pending = buffer.split_off(valid_up_to);
            String::from_utf8_lossy(&buffer).into_owned()
        }
        Err(_) => String::from_utf8_lossy(&buffer).into_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::Utf8Chunks;

    #[test]
    fn preserves_multibyte_char_split_across_chunks() {
        let mut decoder = Utf8Chunks::default();
        let mut output = String::new();

        output.push_str(&decoder.push(vec![b'l', b'i', b'n', b'e', b' ', 0xE6]));
        output.push_str(&decoder.push(vec![0xBC, 0xA2, b'\n']));
        output.push_str(&decoder.flush());

        assert_eq!(output, "line 漢\n");
        assert!(!output.contains('\u{FFFD}'));
    }
}
