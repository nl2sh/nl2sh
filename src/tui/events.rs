use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEvent};
use std::time::{Duration, Instant};
pub fn next() -> Result<Option<Event>> {
    if event::poll(Duration::from_millis(33))? {
        Ok(Some(event::read()?))
    } else {
        Ok(None)
    }
}

#[derive(Default)]
pub(super) struct FragmentedArrowFilter {
    state: ArrowSequenceState,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum WindowsScrollAction {
    ScrollUp(usize),
    ScrollDown(usize),
    InputHistoryUp,
    InputHistoryDown,
}

#[derive(Default)]
pub(super) struct WindowsScrollFilter {
    pending: Option<(KeyCode, Instant)>,
    scrolling: Option<(KeyCode, Instant)>,
}

impl WindowsScrollFilter {
    const START_WINDOW: Duration = Duration::from_millis(60);
    const CONTINUE_WINDOW: Duration = Duration::from_millis(120);

    pub(super) fn push(&mut self, code: KeyCode) -> Option<WindowsScrollAction> {
        if !matches!(code, KeyCode::Up | KeyCode::Down) {
            return None;
        }
        let now = Instant::now();
        if let Some((direction, last)) = self.scrolling {
            if direction == code && now.duration_since(last) <= Self::CONTINUE_WINDOW {
                self.scrolling = Some((code, now));
                return Some(Self::scroll_action(code, 3));
            }
            self.scrolling = None;
        }
        if let Some((direction, started)) = self.pending.take() {
            if direction == code && now.duration_since(started) <= Self::START_WINDOW {
                self.scrolling = Some((code, now));
                return Some(Self::scroll_action(code, 6));
            }
            self.pending = Some((code, now));
            return Some(Self::history_action(direction));
        }
        self.pending = Some((code, now));
        None
    }

    pub(super) fn take_expired(&mut self) -> Option<WindowsScrollAction> {
        if self
            .scrolling
            .is_some_and(|(_, last)| last.elapsed() > Self::CONTINUE_WINDOW)
        {
            self.scrolling = None;
        }
        let (direction, started) = self.pending?;
        if started.elapsed() < Self::START_WINDOW {
            return None;
        }
        self.pending = None;
        Some(Self::history_action(direction))
    }

    fn scroll_action(code: KeyCode, rows: usize) -> WindowsScrollAction {
        match code {
            KeyCode::Up => WindowsScrollAction::ScrollUp(rows),
            _ => WindowsScrollAction::ScrollDown(rows),
        }
    }

    fn history_action(code: KeyCode) -> WindowsScrollAction {
        match code {
            KeyCode::Up => WindowsScrollAction::InputHistoryUp,
            _ => WindowsScrollAction::InputHistoryDown,
        }
    }
}

#[derive(Default)]
enum ArrowSequenceState {
    #[default]
    Idle,
    Escape(Instant),
    Introducer,
}

impl FragmentedArrowFilter {
    pub(super) fn normalize(&mut self, mut key: KeyEvent) -> Option<KeyEvent> {
        match (&self.state, key.code) {
            (_, KeyCode::Esc) => {
                self.state = ArrowSequenceState::Escape(Instant::now());
                None
            }
            (ArrowSequenceState::Escape(_), KeyCode::Char('[' | 'O')) => {
                self.state = ArrowSequenceState::Introducer;
                None
            }
            (ArrowSequenceState::Introducer, KeyCode::Char(character)) => {
                let code = match character {
                    'A' => Some(KeyCode::Up),
                    'B' => Some(KeyCode::Down),
                    'C' => Some(KeyCode::Right),
                    'D' => Some(KeyCode::Left),
                    'H' => Some(KeyCode::Home),
                    'F' => Some(KeyCode::End),
                    // F2 is commonly encoded as the SS3 sequence ESC O Q.
                    'Q' => Some(KeyCode::F(2)),
                    _ => None,
                };
                self.reset();
                if let Some(code) = code {
                    key.code = code;
                }
                Some(key)
            }
            _ => {
                self.reset();
                Some(key)
            }
        }
    }

    pub(super) fn reset(&mut self) {
        self.state = ArrowSequenceState::Idle;
    }

    /// Returns a standalone Escape after allowing fragmented arrow bytes to arrive.
    pub(super) fn take_expired_escape(&mut self, delay: Duration) -> bool {
        let expired = matches!(
            self.state,
            ArrowSequenceState::Escape(started) if started.elapsed() >= delay
        );
        if expired {
            self.reset();
        }
        expired
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossterm::event::KeyModifiers;

    #[test]
    fn reconstructs_fragmented_csi_and_ss3_arrows() {
        let mut filter = FragmentedArrowFilter::default();
        for introducer in ['[', 'O'] {
            assert!(filter
                .normalize(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
                .is_none());
            assert!(filter
                .normalize(KeyEvent::new(KeyCode::Char(introducer), KeyModifiers::NONE,))
                .is_none());
            let key = filter
                .normalize(KeyEvent::new(KeyCode::Char('A'), KeyModifiers::NONE))
                .map(|key| key.code);
            assert_eq!(key, Some(KeyCode::Up));
        }
    }

    #[test]
    fn reconstructs_fragmented_ss3_f2_without_leaking_oq() {
        let mut filter = FragmentedArrowFilter::default();
        assert!(filter
            .normalize(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .is_none());
        assert!(filter
            .normalize(KeyEvent::new(KeyCode::Char('O'), KeyModifiers::NONE))
            .is_none());
        let key = filter
            .normalize(KeyEvent::new(KeyCode::Char('Q'), KeyModifiers::NONE))
            .map(|key| key.code);
        assert_eq!(key, Some(KeyCode::F(2)));
    }

    #[test]
    fn preserves_unrelated_characters() {
        let mut filter = FragmentedArrowFilter::default();
        let key = filter
            .normalize(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE))
            .map(|key| key.code);
        assert_eq!(key, Some(KeyCode::Char('x')));
    }

    #[test]
    fn releases_a_standalone_escape_after_the_grace_period() {
        let mut filter = FragmentedArrowFilter::default();
        assert!(filter
            .normalize(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE))
            .is_none());
        assert!(filter.take_expired_escape(Duration::ZERO));
        assert!(!filter.take_expired_escape(Duration::ZERO));
    }

    #[test]
    fn windows_scroll_filter_preserves_single_arrows_and_detects_wheel_bursts() {
        let mut filter = WindowsScrollFilter::default();
        assert_eq!(filter.push(KeyCode::Up), None);
        std::thread::sleep(WindowsScrollFilter::START_WINDOW + Duration::from_millis(5));
        assert_eq!(
            filter.take_expired(),
            Some(WindowsScrollAction::InputHistoryUp)
        );

        assert_eq!(filter.push(KeyCode::Down), None);
        assert_eq!(
            filter.push(KeyCode::Down),
            Some(WindowsScrollAction::ScrollDown(6))
        );
        assert_eq!(
            filter.push(KeyCode::Down),
            Some(WindowsScrollAction::ScrollDown(3))
        );
    }
}
