mod chat_completions;
mod client;
mod responses;
mod retry;
mod streaming;
mod types;
pub use client::{build_client, HttpLlmClient, LlmClient, TextDeltaSink};
pub use types::*;

#[cfg(test)]
mod input_media_tests {
    use super::*;
    #[test]
    fn both_providers_map_user_images_to_native_parts() {
        let mut user = ConversationMessage::new(Role::User, "inspect image");
        user.attachments = vec![ToolAttachment {
            media_type: "image/jpeg".into(),
            base64_data: "image-data".into(),
        }];
        let request = LlmRequest {
            model: "vision".into(),
            items: vec![ConversationItem::Message(user)],
            tools: Vec::new(),
        };
        assert_eq!(
            chat_completions::request(&request)["messages"][0]["content"][1]["image_url"]["url"],
            "data:image/jpeg;base64,image-data"
        );
        assert_eq!(
            responses::request(&request)["input"][0]["content"][1]["image_url"],
            "data:image/jpeg;base64,image-data"
        );
    }
}
