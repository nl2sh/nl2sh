use anyhow::Result;
use crossterm::{
    cursor::Show,
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{backend::CrosstermBackend, Terminal};
use std::io::{self, Stdout, Write};

/// Best-effort restoration used by panic paths that cannot rely on RAII cleanup.
pub(crate) fn best_effort_restore() {
    let mut out = io::stdout();
    let _ = write_restore_sequences(&mut out);
    let _ = disable_raw_mode();
}

fn write_restore_sequences(out: &mut impl Write) -> io::Result<()> {
    execute!(out, DisableMouseCapture, LeaveAlternateScreen, Show)
}

pub struct TerminalGuard {
    terminal: Terminal<CrosstermBackend<Stdout>>,
    mouse: bool,
}

pub(super) fn windows_scroll_fallback() -> bool {
    std::env::var_os("NL2SH_WINDOWS_SCROLL").is_some()
}

impl TerminalGuard {
    pub fn enter() -> Result<Self> {
        enable_raw_mode()?;
        let mut out = io::stdout();
        if let Err(error) = execute!(out, EnterAlternateScreen) {
            let _ = disable_raw_mode();
            return Err(error.into());
        }
        let mouse = !windows_scroll_fallback();
        if mouse {
            if let Err(error) = execute!(out, EnableMouseCapture) {
                let _ = execute!(out, LeaveAlternateScreen, Show);
                let _ = disable_raw_mode();
                return Err(error.into());
            }
        }
        let terminal = match Terminal::new(CrosstermBackend::new(out)) {
            Ok(terminal) => terminal,
            Err(error) => {
                let mut rollback = io::stdout();
                let _ = execute!(rollback, DisableMouseCapture, LeaveAlternateScreen, Show);
                let _ = disable_raw_mode();
                return Err(error.into());
            }
        };
        Ok(Self { terminal, mouse })
    }
    pub fn terminal(&mut self) -> &mut Terminal<CrosstermBackend<Stdout>> {
        &mut self.terminal
    }
}
impl Drop for TerminalGuard {
    fn drop(&mut self) {
        if self.mouse {
            let _ = write_restore_sequences(self.terminal.backend_mut());
        } else {
            let _ = execute!(self.terminal.backend_mut(), LeaveAlternateScreen, Show);
        }
        let _ = disable_raw_mode();
        let _ = self.terminal.show_cursor();
    }
}

#[cfg(test)]
mod tests {
    use super::write_restore_sequences;

    #[test]
    fn restoration_explicitly_disables_sgr_mouse_capture() -> anyhow::Result<()> {
        let mut output = Vec::new();
        write_restore_sequences(&mut output)?;
        let output = String::from_utf8(output)?;
        assert!(output.contains("\u{1b}[?1000l"));
        assert!(output.contains("\u{1b}[?1006l"));
        Ok(())
    }
}
