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
}
