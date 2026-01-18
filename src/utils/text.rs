pub(crate) trait TextBuffer {
    fn push_str(&mut self, s: &str);
    fn push_char(&mut self, ch: char);
}

impl TextBuffer for String {
    fn push_str(&mut self, s: &str) {
        self.push_str(s);
    }

    fn push_char(&mut self, ch: char) {
        self.push(ch);
    }
}

impl TextBuffer for Vec<u8> {
    fn push_str(&mut self, s: &str) {
        self.extend_from_slice(s.as_bytes());
    }

    fn push_char(&mut self, ch: char) {
        if ch.is_ascii() {
            self.push(ch as u8);
            return;
        }

        let mut buf = [0u8; 4];
        let encoded = ch.encode_utf8(&mut buf);
        self.extend_from_slice(encoded.as_bytes());
    }
}
