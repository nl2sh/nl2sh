use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
/// Provider-neutral conversation role.
pub enum Role {
    /// Immutable behavior instruction.
    System,
    /// User-authored content.
    User,
    /// Model-authored content.
    Assistant,
    /// Tool output content.
    Tool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// A provider-neutral text conversation message.
pub struct ConversationMessage {
    /// Author role.
    pub role: Role,
    /// Text body.
    pub content: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    /// Tool call identifier when this is a tool message.
    pub tool_call_id: Option<String>,
}

impl ConversationMessage {
    /// Creates a plain text message without a tool identifier.
    pub fn new(role: Role, content: impl Into<String>) -> Self {
        Self {
            role,
            content: content.into(),
            tool_call_id: None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// JSON-schema definition advertised to the model.
pub struct ToolDefinition {
    /// Function name.
    pub name: String,
    /// Model-facing behavior description.
    pub description: String,
    /// JSON Schema for function arguments.
    pub parameters: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// A normalized function call requested by a model.
pub struct ToolCall {
    /// Provider call identifier.
    pub id: String,
    /// Function name.
    pub name: String,
    /// Parsed JSON arguments.
    pub arguments: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// Real output produced while handling one tool call.
pub struct ToolResult {
    /// Identifier of the corresponding call.
    pub call_id: String,
    /// Text returned to the model.
    pub output: String,
    /// Whether execution succeeded.
    pub success: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
/// One ordered model-call/tool-output interaction unit.
pub struct ToolRound {
    /// Calls emitted together by the assistant.
    pub calls: Vec<ToolCall>,
    /// Results corresponding to those calls.
    pub results: Vec<ToolResult>,
}

/// One ordered provider-neutral conversation item.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum ConversationItem {
    /// A system, user, or assistant text message.
    Message(ConversationMessage),
    /// An inseparable assistant function-call/tool-output round.
    Tools(ToolRound),
}

#[derive(Debug, Clone)]
/// Provider-neutral completion request.
pub struct LlmRequest {
    /// Provider model identifier.
    pub model: String,
    /// Ordered text and complete tool history.
    pub items: Vec<ConversationItem>,
    /// Tools available in this request.
    pub tools: Vec<ToolDefinition>,
}

#[derive(Debug, Clone, Default)]
/// Token accounting returned when a provider supplies it.
pub struct Usage {
    /// Input token count.
    pub input_tokens: Option<u64>,
    /// Output token count.
    pub output_tokens: Option<u64>,
}

impl Usage {
    /// Adds provider-reported counters while preserving unknown values.
    pub fn accumulate(&mut self, other: &Self) {
        self.input_tokens = add_optional(self.input_tokens, other.input_tokens);
        self.output_tokens = add_optional(self.output_tokens, other.output_tokens);
    }

    /// Returns the reported total when both input and output are known.
    pub fn total_tokens(&self) -> Option<u64> {
        Some(self.input_tokens?.saturating_add(self.output_tokens?))
    }
}

fn add_optional(current: Option<u64>, next: Option<u64>) -> Option<u64> {
    match (current, next) {
        (Some(current), Some(next)) => Some(current.saturating_add(next)),
        (None, Some(next)) => Some(next),
        (Some(current), None) => Some(current),
        (None, None) => None,
    }
}

#[cfg(test)]
mod usage_tests {
    use super::Usage;

    #[test]
    fn accumulates_usage_across_agent_steps() {
        let mut usage = Usage::default();
        usage.accumulate(&Usage {
            input_tokens: Some(10),
            output_tokens: Some(4),
        });
        usage.accumulate(&Usage {
            input_tokens: Some(7),
            output_tokens: Some(3),
        });
        assert_eq!(usage.input_tokens, Some(17));
        assert_eq!(usage.output_tokens, Some(7));
        assert_eq!(usage.total_tokens(), Some(24));
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Normalized reason a generation ended.
pub enum FinishReason {
    /// Model produced its final response.
    Stop,
    /// Model requested one or more tools.
    ToolCalls,
    /// Provider length limit was reached.
    Length,
    /// Provider-specific reason.
    Other(String),
}

#[derive(Debug, Clone)]
/// Provider-neutral completion response.
pub struct LlmResponse {
    /// Optional model text.
    pub text: Option<String>,
    /// Normalized function calls.
    pub tool_calls: Vec<ToolCall>,
    /// Optional usage accounting.
    pub usage: Usage,
    /// Normalized finish reason.
    pub finish_reason: FinishReason,
}
