use super::{analyzer::visit_list, effects::Effects};
use crate::security::RiskLevel;
use brush_parser::{Parser, ParserOptions};
use std::io::Cursor;

pub(super) fn visit_source(source: &str, effects: &mut Effects, depth: usize) {
    if depth >= 16 || source.len() > 64 * 1024 {
        effects.raise(
            RiskLevel::Dangerous,
            "shell-analysis-limit",
            "shell analysis limit exceeded",
        );
        return;
    }
    let mut parser = Parser::new(Cursor::new(source.as_bytes()), &ParserOptions::default());
    match parser.parse_program() {
        Ok(program) => {
            for command in &program.complete_commands {
                visit_list(command, effects, depth);
            }
        }
        Err(_) => effects.raise(
            RiskLevel::Dangerous,
            "shell-parse-unknown",
            "shell syntax could not be analyzed",
        ),
    }
}
