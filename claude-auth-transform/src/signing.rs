use sha2::{Digest, Sha256};

use crate::bodies::{Message, MessageContent};

const BILLING_SALT: &str = "59cf53e54c78";

pub fn build_billing_header_value(messages: &[Message], version: &str, entrypoint: &str) -> String {
    let text = extract_first_user_message_text(messages);
    let cch = compute_cch(text);
    let suffix = compute_version_suffix(text, version);

    format!(
        "x-anthropic-billing-header: cc_version={version}.{suffix}; cc_entrypoint={entrypoint}; cch={cch};"
    )
}

/// Extract text from the first user message's first text block.
/// Matches Claude Code's `K19()` function exactly: find the first message
/// with role "user", then return the text of its first text content block.
fn extract_first_user_message_text(messages: &[Message]) -> &str {
    let user_msg = messages
        .iter()
        .find(|msg| msg.role == Some("user".to_string()));

    let content = user_msg.and_then(|msg| msg.content.as_ref());
    match content {
        Some(MessageContent::Text(text)) => text,
        Some(MessageContent::Blocks(blocks)) => {
            let text_block = blocks
                .iter()
                .find(|block| block.r#type == Some("text".to_string()));
            text_block
                .and_then(|block| block.text.as_deref())
                .unwrap_or("")
        }
        _ => "",
    }
}

/// Compute cch: first 5 hex characters of SHA-256(messageText).
fn compute_cch(text: &str) -> String {
    let mut sha = Sha256::new();
    sha.update(text.as_bytes());
    hex::encode(sha.finalize())[..5].to_string()
}

/// Compute the 3-char version suffix.
/// Samples characters at indices 4, 7, 20 from the message text (padding
/// with "0" when the message is shorter), then hashes with the billing salt
/// and version string.
fn compute_version_suffix(text: &str, version: &str) -> String {
    let sampled = [4, 7, 20]
        .into_iter()
        .map(|i| text.chars().nth(i).unwrap_or('0'))
        .collect::<String>();

    let input = format!("{BILLING_SALT}{sampled}{version}");

    let mut sha = Sha256::new();
    sha.update(input.as_bytes());
    hex::encode(sha.finalize())[..3].to_string()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn parse_messages(value: serde_json::Value) -> Vec<Message> {
        serde_json::from_value(value).expect("valid Message JSON")
    }

    #[test]
    fn extracts_string_content_from_user_message() {
        let messages = parse_messages(json!([{ "role": "user", "content": "hello" }]));
        assert_eq!(extract_first_user_message_text(&messages), "hello");
    }

    #[test]
    fn extracts_first_text_block_from_array_content() {
        let messages = parse_messages(json!([{
            "role": "user",
            "content": [
                { "type": "text", "text": "first" },
                { "type": "text", "text": "second" }
            ]
        }]));
        assert_eq!(extract_first_user_message_text(&messages), "first");
    }

    #[test]
    fn skips_non_user_messages() {
        let messages = parse_messages(json!([
            { "role": "assistant", "content": "hi" },
            { "role": "user", "content": "hello" }
        ]));
        assert_eq!(extract_first_user_message_text(&messages), "hello");
    }

    #[test]
    fn returns_empty_string_when_no_user_message() {
        let messages = parse_messages(json!([{ "role": "assistant", "content": "hi" }]));
        assert_eq!(extract_first_user_message_text(&messages), "");
    }

    #[test]
    fn returns_empty_string_for_empty_messages_array() {
        let messages: Vec<Message> = Vec::new();
        assert_eq!(extract_first_user_message_text(&messages), "");
    }

    #[test]
    fn returns_empty_string_when_no_text_blocks_in_array_content() {
        let messages = parse_messages(json!([{
            "role": "user",
            "content": [{ "type": "image" }]
        }]));
        assert_eq!(extract_first_user_message_text(&messages), "");
    }

    #[test]
    fn compute_cch_matches_hey_vector() {
        assert_eq!(compute_cch("hey"), "fa690");
    }

    #[test]
    fn compute_cch_matches_empty_string_vector() {
        assert_eq!(compute_cch(""), "e3b0c");
    }

    #[test]
    fn compute_cch_matches_long_message_vector() {
        assert_eq!(compute_cch("Hello, how are you doing today?"), "852db");
    }

    #[test]
    fn compute_version_suffix_matches_hey_v2137_vector() {
        assert_eq!(compute_version_suffix("hey", "2.1.37"), "0d9");
    }

    #[test]
    fn compute_version_suffix_matches_hey_v2190_vector() {
        assert_eq!(compute_version_suffix("hey", "2.1.90"), "b39");
    }

    #[test]
    fn compute_version_suffix_pads_short_messages() {
        assert_eq!(compute_version_suffix("hey", "2.1.37"), "0d9");
    }

    #[test]
    fn compute_version_suffix_samples_expected_indices_for_long_message() {
        assert_eq!(
            compute_version_suffix("Hello, how are you doing today?", "2.1.90"),
            "494"
        );
    }

    #[test]
    fn compute_version_suffix_handles_empty_string() {
        let result = compute_version_suffix("", "2.1.90");
        assert_eq!(result.len(), 3);
    }

    #[test]
    fn build_billing_header_value_for_simple_string_message() {
        let messages = parse_messages(json!([{ "role": "user", "content": "hey" }]));
        let result = build_billing_header_value(&messages, "2.1.90", "cli");
        assert_eq!(
            result,
            "x-anthropic-billing-header: cc_version=2.1.90.b39; cc_entrypoint=cli; cch=fa690;"
        );
    }

    #[test]
    fn build_billing_header_value_uses_first_text_block_from_array_content() {
        let messages = parse_messages(json!([{
            "role": "user",
            "content": [
                { "type": "text", "text": "hey" },
                { "type": "text", "text": "ignored" }
            ]
        }]));
        let result = build_billing_header_value(&messages, "2.1.90", "cli");
        assert_eq!(
            result,
            "x-anthropic-billing-header: cc_version=2.1.90.b39; cc_entrypoint=cli; cch=fa690;"
        );
    }

    #[test]
    fn build_billing_header_value_handles_missing_user_message() {
        let messages: Vec<Message> = Vec::new();
        let result = build_billing_header_value(&messages, "2.1.90", "cli");
        assert!(result.contains("cch=e3b0c"));
    }

    #[test]
    fn build_billing_header_value_uses_provided_entrypoint() {
        let messages = parse_messages(json!([{ "role": "user", "content": "hey" }]));
        let result = build_billing_header_value(&messages, "2.1.90", "sdk-cli");
        assert!(result.contains("cc_entrypoint=sdk-cli"));
    }
}
