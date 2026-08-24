#[derive(Default)]
pub struct Input {
    pub text: String,
    cursor: usize,
}
impl Input {
    pub fn push(&mut self, c: char) {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        if matches!(c, 'M' | 'm') {
            self.remove_sgr_mouse_report();
        }
    }
    pub fn backspace(&mut self) {
        if let Some((previous, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.text.drain(previous..self.cursor);
            self.cursor = previous;
        }
    }
    pub fn delete(&mut self) {
        if let Some(character) = self.text[self.cursor..].chars().next() {
            self.text
                .drain(self.cursor..self.cursor + character.len_utf8());
        }
    }
    pub fn move_left(&mut self) {
        if let Some((previous, _)) = self.text[..self.cursor].char_indices().next_back() {
            self.cursor = previous;
        }
    }
    pub fn move_right(&mut self) {
        if let Some(character) = self.text[self.cursor..].chars().next() {
            self.cursor += character.len_utf8();
        }
    }
    pub fn move_home(&mut self) {
        self.cursor = 0;
    }
    pub fn move_end(&mut self) {
        self.cursor = self.text.len();
    }
    pub fn cursor(&self) -> usize {
        self.cursor
    }
    pub fn set(&mut self, value: String) {
        self.text = value;
        self.cursor = self.text.len();
    }
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    fn remove_sgr_mouse_report(&mut self) {
        if strip_trailing_sgr_mouse_report(&mut self.text) {
            self.cursor = self.text.len();
        }
    }
}

/// Removes a complete trailing SGR mouse report, including the degraded form
/// seen when an adb terminal drops the leading CSI bytes.
pub(super) fn strip_trailing_sgr_mouse_report(text: &mut String) -> bool {
    if !matches!(text.chars().next_back(), Some('M' | 'm')) {
        return false;
    }
    let Some(angle) = text.rfind('<') else {
        return false;
    };
    let candidate = &text[angle + 1..text.len().saturating_sub(1)];
    let mut fields = candidate.split(';');
    let valid = (0..3).all(|_| {
        fields
            .next()
            .is_some_and(|field| !field.is_empty() && field.chars().all(|c| c.is_ascii_digit()))
    }) && fields.next().is_none();
    if !valid {
        return false;
    }
    let start = angle
        .checked_sub(1)
        .filter(|index| text.as_bytes().get(*index) == Some(&b'['))
        .unwrap_or(angle);
    text.truncate(start);
    true
}

impl From<&str> for Input {
    fn from(value: &str) -> Self {
        Self {
            text: value.into(),
            cursor: value.len(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Input;

    #[test]
    fn strips_fragmented_sgr_mouse_reports_from_user_input() {
        let mut input = Input::from("保留这些文字");
        for character in "[<35;96;3M".chars() {
            input.push(character);
        }
        assert_eq!(input.text, "保留这些文字");

        for character in "<35;46;8M".chars() {
            input.push(character);
        }
        assert_eq!(input.text, "保留这些文字");
    }

    #[test]
    fn preserves_normal_bracketed_input() {
        let mut input = Input::default();
        for character in "echo [<not-a-mouse>]".chars() {
            input.push(character);
        }
        assert_eq!(input.text, "echo [<not-a-mouse>]");
        for character in " <35;46;XM".chars() {
            input.push(character);
        }
        assert_eq!(input.text, "echo [<not-a-mouse>] <35;46;XM");
    }

    #[test]
    fn edits_unicode_text_at_the_cursor() {
        let mut input = Input::from("你a");
        input.move_left();
        input.push('好');
        assert_eq!(input.text, "你好a");
        input.backspace();
        assert_eq!(input.text, "你a");
        input.move_home();
        input.delete();
        assert_eq!(input.text, "a");
        input.move_end();
        assert_eq!(input.cursor(), 1);
    }
}
