#[derive(Default)]
pub struct Input {
    pub text: String,
    cursor: usize,
}
impl Input {
    pub(super) fn push(&mut self, c: char) -> Option<SgrMouseReport> {
        self.text.insert(self.cursor, c);
        self.cursor += c.len_utf8();
        if matches!(c, 'M' | 'm') {
            return self.remove_sgr_mouse_report();
        }
        None
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
    pub fn replace_before_cursor(&mut self, start: usize, value: &str) -> bool {
        if start > self.cursor || !self.text.is_char_boundary(start) {
            return false;
        }
        self.text.replace_range(start..self.cursor, value);
        self.cursor = start.saturating_add(value.len());
        true
    }
    pub fn clear(&mut self) {
        self.text.clear();
        self.cursor = 0;
    }
    pub fn take(&mut self) -> String {
        self.cursor = 0;
        std::mem::take(&mut self.text)
    }

    fn remove_sgr_mouse_report(&mut self) -> Option<SgrMouseReport> {
        let report = take_trailing_sgr_mouse_report(&mut self.text);
        if report.is_some() {
            self.cursor = self.text.len();
        }
        report
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SgrMouseReport {
    ScrollUp,
    ScrollDown,
    Other,
}

/// Removes a complete trailing SGR mouse report, including the degraded form
/// seen when an adb terminal drops the leading CSI bytes.
pub(super) fn strip_trailing_sgr_mouse_report(text: &mut String) -> bool {
    take_trailing_sgr_mouse_report(text).is_some()
}

/// Removes and classifies a complete trailing SGR mouse report. Some Windows
/// adb terminal paths deliver these bytes as ordinary key events instead of a
/// crossterm mouse event.
pub(super) fn take_trailing_sgr_mouse_report(text: &mut String) -> Option<SgrMouseReport> {
    if !matches!(text.chars().next_back(), Some('M' | 'm')) {
        return None;
    }
    let angle = text.rfind('<')?;
    let candidate = &text[angle + 1..text.len().saturating_sub(1)];
    let mut fields = candidate.split(';');
    let button = fields.next().and_then(|field| field.parse::<u16>().ok())?;
    let column = fields.next().and_then(|field| field.parse::<u16>().ok())?;
    let row = fields.next().and_then(|field| field.parse::<u16>().ok())?;
    if fields.next().is_some() || column == 0 || row == 0 {
        return None;
    }
    let start = angle
        .checked_sub(1)
        .filter(|index| text.as_bytes().get(*index) == Some(&b'['))
        .unwrap_or(angle);
    text.truncate(start);

    // Bits 2-4 are keyboard modifiers and bit 5 is the motion flag. Some
    // Windows terminal/adb paths retain the motion flag on wheel reports,
    // producing 96/97 instead of the canonical 64/65.
    match button & !(4 | 8 | 16 | 32) {
        64 => Some(SgrMouseReport::ScrollUp),
        65 => Some(SgrMouseReport::ScrollDown),
        _ => Some(SgrMouseReport::Other),
    }
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
    use super::{Input, SgrMouseReport};

    #[test]
    fn strips_fragmented_sgr_mouse_reports_from_user_input() {
        let mut input = Input::from("保留这些文字");
        for character in "[<35;96;3M".chars() {
            let _ = input.push(character);
        }
        assert_eq!(input.text, "保留这些文字");

        for character in "<35;46;8M".chars() {
            let _ = input.push(character);
        }
        assert_eq!(input.text, "保留这些文字");
    }

    #[test]
    fn classifies_fragmented_vertical_wheel_reports() {
        let mut input = Input::from("保留这些文字");
        let mut report = None;
        for character in "<64;46;8M".chars() {
            if let Some(parsed) = input.push(character) {
                report = Some(parsed);
            }
        }
        assert_eq!(report, Some(SgrMouseReport::ScrollUp));
        assert_eq!(input.text, "保留这些文字");

        for character in "[<69;46;8M".chars() {
            if let Some(parsed) = input.push(character) {
                report = Some(parsed);
            }
        }
        assert_eq!(report, Some(SgrMouseReport::ScrollDown));
        assert_eq!(input.text, "保留这些文字");

        for (sequence, expected) in [
            ("<96;46;8M", SgrMouseReport::ScrollUp),
            ("<97;46;8M", SgrMouseReport::ScrollDown),
        ] {
            report = None;
            for character in sequence.chars() {
                if let Some(parsed) = input.push(character) {
                    report = Some(parsed);
                }
            }
            assert_eq!(report, Some(expected));
            assert_eq!(input.text, "保留这些文字");
        }
    }

    #[test]
    fn preserves_normal_bracketed_input() {
        let mut input = Input::default();
        for character in "echo [<not-a-mouse>]".chars() {
            let _ = input.push(character);
        }
        assert_eq!(input.text, "echo [<not-a-mouse>]");
        for character in " <35;46;XM".chars() {
            let _ = input.push(character);
        }
        assert_eq!(input.text, "echo [<not-a-mouse>] <35;46;XM");
    }

    #[test]
    fn edits_unicode_text_at_the_cursor() {
        let mut input = Input::from("你a");
        input.move_left();
        let _ = input.push('好');
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
