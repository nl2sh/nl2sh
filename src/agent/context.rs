use crate::llm::{ConversationItem, ConversationMessage, Role};
/// Bounded text history that removes only complete interaction units.
pub struct ConversationContext {
    system: ConversationItem,
    turns: Vec<Vec<ConversationItem>>,
    max: usize,
}
impl ConversationContext {
    /// Creates a context whose system instruction is never truncated.
    pub fn new(system: impl Into<String>, max: usize) -> Self {
        Self {
            system: ConversationItem::Message(ConversationMessage::new(Role::System, system)),
            turns: Vec::new(),
            max,
        }
    }
    /// Adds one complete user/assistant interaction and evicts oldest units.
    pub fn push_turn(&mut self, messages: Vec<ConversationItem>) {
        self.turns.push(messages);
        while self.turns.len() > self.max {
            self.turns.remove(0);
        }
    }
    /// Returns the system message followed by retained complete interactions.
    pub fn items(&self) -> Vec<ConversationItem> {
        std::iter::once(self.system.clone())
            .chain(self.turns.iter().flatten().cloned())
            .collect()
    }

    /// Shrinks retained history using the provider's observed input-token count.
    ///
    /// Only complete prior turns are removed. The system instruction and the
    /// current in-flight turn remain outside this eviction boundary.
    pub fn trim_for_observed_usage(&mut self, observed: u64, budget: u64) -> usize {
        if observed <= budget || self.turns.is_empty() {
            return 0;
        }
        let retained = self.turns.len();
        let desired = ((retained as u128 * budget as u128) / observed as u128) as usize;
        let remove = retained.saturating_sub(desired).max(1).min(retained);
        self.turns.drain(0..remove);
        remove
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn observed_usage_removes_only_oldest_complete_turns() {
        let mut context = ConversationContext::new("system", 4);
        for value in ["one", "two", "three", "four"] {
            context.push_turn(vec![ConversationItem::Message(ConversationMessage::new(
                Role::User,
                value,
            ))]);
        }

        assert_eq!(context.trim_for_observed_usage(100, 50), 2);
        let retained = context
            .items()
            .into_iter()
            .filter_map(|item| match item {
                ConversationItem::Message(message) => Some(message.content),
                ConversationItem::Tools(_) => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(retained, ["system", "three", "four"]);
    }
}
