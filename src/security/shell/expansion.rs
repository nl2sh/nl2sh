use super::{effects::Effects, parser::visit_source};
use crate::security::RiskLevel;
use brush_parser::{
    ast,
    word::{self, ParameterExpr, WordPiece},
    ParserOptions,
};

pub(super) fn visit_word(word: &ast::Word, effects: &mut Effects, depth: usize) {
    match word::parse(&word.value, &ParserOptions::default()) {
        Ok(pieces) => visit_pieces(&pieces, effects, depth),
        Err(_) => effects.raise(
            RiskLevel::Dangerous,
            "shell-word-unknown",
            "shell word could not be analyzed",
        ),
    }
}

pub(super) fn visit_pieces(
    pieces: &[word::WordPieceWithSource],
    effects: &mut Effects,
    depth: usize,
) {
    for piece in pieces {
        match &piece.piece {
            WordPiece::CommandSubstitution(source)
            | WordPiece::BackquotedCommandSubstitution(source) => {
                visit_source(source, effects, depth + 1);
            }
            WordPiece::DoubleQuotedSequence(inner)
            | WordPiece::GettextDoubleQuotedSequence(inner) => {
                visit_pieces(inner, effects, depth);
            }
            WordPiece::ParameterExpansion(parameter) => {
                let nested = match parameter {
                    ParameterExpr::UseDefaultValues { default_value, .. }
                    | ParameterExpr::AssignDefaultValues { default_value, .. } => {
                        default_value.as_deref()
                    }
                    ParameterExpr::IndicateErrorIfNullOrUnset { error_message, .. } => {
                        error_message.as_deref()
                    }
                    ParameterExpr::UseAlternativeValue {
                        alternative_value, ..
                    } => alternative_value.as_deref(),
                    _ => None,
                };
                if let Some(nested) = nested {
                    visit_word(&ast::Word::new(nested), effects, depth + 1);
                }
                if matches!(parameter, ParameterExpr::AssignDefaultValues { .. }) {
                    effects.raise(
                        RiskLevel::Mutating,
                        "shell-parameter-assignment",
                        "shell parameter assignment",
                    );
                }
            }
            _ => {}
        }
    }
}

pub(super) fn static_word(raw: &str) -> Option<String> {
    let pieces = word::parse(raw, &ParserOptions::default()).ok()?;
    let mut value = String::new();
    for piece in pieces {
        match piece.piece {
            WordPiece::Text(text)
            | WordPiece::SingleQuotedText(text)
            | WordPiece::AnsiCQuotedText(text)
            | WordPiece::EscapeSequence(text) => {
                value.push_str(text.strip_prefix('\\').unwrap_or(&text));
            }
            WordPiece::DoubleQuotedSequence(inner) => {
                for part in inner {
                    match part.piece {
                        WordPiece::Text(text) => value.push_str(&text),
                        WordPiece::EscapeSequence(text) => {
                            value.push_str(text.strip_prefix('\\').unwrap_or(&text));
                        }
                        _ => return None,
                    }
                }
            }
            _ => return None,
        }
    }
    Some(value)
}
