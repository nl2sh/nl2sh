//! Shared short-title generation for saved Agent conversations.

use crate::{
    config::Config,
    llm::{ConversationItem, ConversationMessage, LlmClient, LlmRequest, Role},
};

/// Generates a bounded, user-facing title without exposing tools or changing the Agent transcript.
pub async fn generate_title(
    llm: &dyn LlmClient,
    cfg: &Config,
    input: &str,
    answer: &str,
) -> Option<String> {
    let request = LlmRequest {
        model: cfg.model.clone(),
        items: vec![
            ConversationItem::Message(ConversationMessage::new(Role::System, "Generate a concise title for this conversation. Use the user's language. Output only the title, at most 12 Chinese characters or 8 words. Do not use quotes or punctuation at the ends.")),
            ConversationItem::Message(ConversationMessage::new(Role::User, format!("User: {}\nAssistant: {}", crate::limits::truncate_text(input, 1000), crate::limits::truncate_text(answer, 1000)))),
        ],
        tools: Vec::new(),
    };
    let response = llm.complete(request).await.ok()?;
    normalize_title(response.text.as_deref()?)
}

pub(crate) fn normalize_title(value: &str) -> Option<String> {
    let title = value
        .lines()
        .next()?
        .trim()
        .trim_matches(|c| matches!(c, '"' | '\'' | '`' | '#' | '*' | '：' | ':'))
        .trim();
    if title.is_empty() {
        return None;
    }
    let mut bounded = String::new();
    for character in title.chars().take(48) {
        if bounded.len() + character.len_utf8() > 150 {
            break;
        }
        bounded.push(character);
    }
    (!bounded.is_empty()).then_some(bounded)
}

#[cfg(test)]
mod tests {
    use super::normalize_title;

    #[test]
    fn generated_titles_are_bounded_and_cleaned() {
        assert_eq!(
            normalize_title("**\"检查网络连接\"**\nextra"),
            Some("检查网络连接".into())
        );
        assert_eq!(normalize_title("   "), None);
        assert_eq!(
            normalize_title(&"a".repeat(60)).map(|value| value.chars().count()),
            Some(48)
        );
        assert!(normalize_title(&"😀".repeat(60)).is_some_and(|value| value.len() <= 150));
    }
}
