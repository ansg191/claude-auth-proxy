use std::collections::{HashMap, HashSet};

use tracing::{debug, trace};

use crate::{
    Error, TransformConfig,
    bodies::{Message, MessageBody, MessageContent, SystemEntry},
    config::CONFIG,
    signing::build_billing_header_value,
    tool_names::ToolNameMapper,
};

const SYSTEM_IDENTITY: &str = "You are Claude Code, Anthropic's official CLI for Claude.";
const BILLING_PREFIX: &str = "x-anthropic-billing-header";

pub fn transform_body(
    bytes: &[u8],
    config: &TransformConfig,
    tool_name_mapper: &ToolNameMapper,
) -> Result<Vec<u8>, Error> {
    let Ok(mut parsed): Result<MessageBody, _> = serde_json::from_slice(bytes) else {
        debug!("Failed to parse body, skipping transformation");
        return Ok(bytes.to_vec());
    };

    inject_billing_header(&mut parsed, config);
    ensure_identity_prefix(&mut parsed);
    split_identity_entries(&mut parsed);
    relocate_non_core_system_entries(&mut parsed);

    // Strip effort for models that don't support it (e.g. haiku).
    // OpenCode sends { output_config: { effort: "high" } } but haiku
    // rejects the effort parameter with a 400 error.
    let override_ = parsed
        .model
        .as_deref()
        .and_then(|model| CONFIG.get_model_override(model));
    if let Some(override_) = override_
        && override_.disable_effort
    {
        trace!(model = parsed.model, "Disabling effort for model");
        parsed.output_config.remove("effort");
        parsed.thinking.remove("effort");
    }

    // Obfuscate all tool names to avoid name-based upstream validation.
    parsed.tools.iter_mut().for_each(|tool| {
        if let Some(name) = tool.name.as_mut() {
            *name = tool_name_mapper.obfuscate(name);
        }
    });

    if let Some(tool_choice) = parsed.tool_choice.as_mut()
        && let Some(name) = tool_choice.name.as_mut()
    {
        *name = tool_name_mapper.obfuscate(name);
    }

    for message in &mut parsed.messages {
        if let Some(MessageContent::Blocks(blocks)) = message.content.as_mut() {
            for block in blocks.iter_mut() {
                if let Some(tp) = block.r#type.as_deref()
                    && tp == "tool_use"
                    && let Some(name) = block.extra.get_mut("name")
                    && name.is_string()
                {
                    *name = tool_name_mapper
                        .obfuscate(name.as_str().expect("name already checked as string"))
                        .into();
                }
            }
        }
    }

    repair_tool_pairs(&mut parsed.messages);

    serde_json::to_vec(&parsed).map_err(Error::Json)
}

// --- Billing header: inject as system[0] (no cache_control) ---
fn inject_billing_header(parsed: &mut MessageBody, config: &TransformConfig) {
    let version = &config.cc_version;
    let entrypoint = &config.entrypoint;
    let billing_header = build_billing_header_value(&parsed.messages, version, entrypoint);
    trace!(billing_header, "Computed billing header");

    // Remove any existing billing header entries
    parsed.system.retain(|e| {
        !(e.r#type.as_deref() == Some("text")
            && e.text
                .as_deref()
                .is_some_and(|t| t.starts_with("x-anthropic-billing-header")))
    });

    // Insert billing header as system[0], without cache_control
    parsed.system.insert(
        0,
        SystemEntry {
            r#type: Some("text".to_owned()),
            text: Some(billing_header),
            extra: HashMap::default(),
        },
    );
}

// --- Ensure identity prefix is present ---
// Upstream's opencode plugin adds this via experimental.chat.system.transform
// BEFORE transformBody runs. We're a standalone proxy with no such hook, so
// we inject it here. Anthropic's OAuth validation requires the exact string
// `You are Claude Code, Anthropic's official CLI for Claude.` in system[].
fn ensure_identity_prefix(parsed: &mut MessageBody) {
    let has_identity = parsed.system.iter().any(|entry| {
        entry
            .text
            .as_deref()
            .is_some_and(|t| t.contains(SYSTEM_IDENTITY))
    });
    if !has_identity {
        trace!("Identity prefix not found in system entries, injecting");
        parsed.system.insert(
            1,
            SystemEntry {
                r#type: Some("text".to_owned()),
                text: Some(SYSTEM_IDENTITY.to_owned()),
                extra: HashMap::default(),
            },
        );
    }
}

// --- Split identity prefix into its own system entry ---
// OpenCode's system.transform hook prepends the identity string, but
// OpenCode then concatenates all system entries into a single text block.
// Anthropic's API requires the identity string as a separate entry for
// OAuth validation (see issue griffinmartin/opencode-claude-auth#98).
fn split_identity_entries(parsed: &mut MessageBody) {
    let mut split_system = Vec::with_capacity(parsed.system.len() + 1);
    for entry in parsed.system.drain(..) {
        if let Some(r#type) = entry.r#type.as_deref()
            && r#type == "text"
            && let Some(text) = entry.text.as_deref()
            && text.starts_with(SYSTEM_IDENTITY)
            && text.len() > SYSTEM_IDENTITY.len()
        {
            let rest = &text[SYSTEM_IDENTITY.len()..].trim_start_matches('\n');

            // Preserve all properties except text (e.g. cache_control)
            let mut props = entry.extra;
            // Only keep cache_control on the remainder block to avoid exceeding
            // the API limit of 4 cache_control blocks per request.
            let cache_control = props.remove("cache_control");

            // Push identity
            split_system.push(SystemEntry {
                r#type: Some("text".to_owned()),
                text: Some(SYSTEM_IDENTITY.to_owned()),
                extra: props.clone(),
            });

            if !rest.is_empty() {
                // Push remainder
                if let Some(cc) = cache_control {
                    props.insert("cache_control".to_owned(), cc);
                }
                split_system.push(SystemEntry {
                    r#type: Some("text".to_owned()),
                    text: Some(rest.to_string()),
                    extra: props,
                });
            }
        } else {
            split_system.push(entry);
        }
    }
    parsed.system = split_system;
}

// --- Relocate non-core system entries to user messages ---
// Anthropic's API now validates the system prompt for OAuth-authenticated
// requests that use Claude Code billing.  Third-party system prompts
// (like OpenCode's) trigger a 400 "out of extra usage" rejection when
// they appear inside the system[] array alongside the identity prefix.
//
// Work-around: keep only the billing header and identity prefix in
// system[], and prepend all other system content to the first user
// message where it is functionally equivalent but avoids the check.
fn relocate_non_core_system_entries(parsed: &mut MessageBody) {
    let mut kept_system = Vec::with_capacity(2);
    let mut moved_texts = Vec::with_capacity(parsed.system.len());

    for entry in &parsed.system {
        if let Some(txt) = entry.text.as_deref()
            && (txt.starts_with(BILLING_PREFIX) || txt.starts_with(SYSTEM_IDENTITY))
        {
            kept_system.push(entry.clone());
        } else if let Some(txt) = entry.text.as_deref()
            && !txt.is_empty()
        {
            moved_texts.push(txt.to_owned());
        }
    }
    if !moved_texts.is_empty() {
        trace!(
            texts_moved = moved_texts.len(),
            "system entries moved to user messages"
        );
        let first_user = parsed
            .messages
            .iter_mut()
            .find(|m| m.role == Some("user".to_string()));
        if let Some(first_user) = first_user {
            parsed.system = kept_system;
            let prefix = moved_texts.join("\n\n");
            match first_user.content {
                Some(MessageContent::Text(ref mut txt)) => {
                    *txt = format!("{prefix}\n\n{txt}");
                }
                Some(MessageContent::Blocks(ref mut blocks)) => {
                    blocks.insert(
                        0,
                        SystemEntry {
                            r#type: Some("text".to_owned()),
                            text: Some(prefix),
                            extra: HashMap::default(),
                        },
                    );
                }
                None => unimplemented!("no text content"),
            }
        }
    }
}

fn repair_tool_pairs(messages: &mut Vec<Message>) {
    let mut tool_use_ids = HashSet::new();
    let mut tool_result_ids = HashSet::new();

    for msg in messages.iter_mut() {
        let Some(MessageContent::Blocks(blocks)) = msg.content.as_mut() else {
            continue;
        };
        for block in blocks.iter_mut() {
            let id = block.extra.get("id");
            if let Some(id) = id
                && let Some(id) = id.as_str()
                && let Some(tp) = block.r#type.as_deref()
                && tp == "tool_use"
            {
                tool_use_ids.insert(id);
            }

            let tool_use_id = block.extra.get("tool_use_id");
            if let Some(tool_use_id) = tool_use_id
                && let Some(tool_use_id) = tool_use_id.as_str()
                && let Some(tp) = block.r#type.as_deref()
                && tp == "tool_result"
            {
                tool_result_ids.insert(tool_use_id);
            }
        }
    }

    // Find orphaned IDs
    let orphaned_uses = tool_use_ids
        .difference(&tool_result_ids)
        .map(ToString::to_string)
        .collect::<HashSet<_>>();
    let orphaned_results = tool_result_ids
        .difference(&tool_use_ids)
        .map(ToString::to_string)
        .collect::<HashSet<_>>();

    // Return early if nothing to fix
    if orphaned_uses.is_empty() && orphaned_results.is_empty() {
        return;
    }

    // Filter orphaned blocks and remove messages with empty content arrays
    for msg in messages.iter_mut() {
        let Some(MessageContent::Blocks(blocks)) = msg.content.as_mut() else {
            continue;
        };

        blocks.retain(|block| {
            let id = block.extra.get("id");
            if let Some(id) = id
                && let Some(id) = id.as_str()
            {
                return !orphaned_uses.contains(id);
            }

            let tool_use_id = block.extra.get("tool_use_id");
            if let Some(tool_use_id) = tool_use_id
                && let Some(tool_use_id) = tool_use_id.as_str()
            {
                return !orphaned_results.contains(tool_use_id);
            }

            true
        });
    }

    messages.retain(|msg| match msg.content {
        None => false,
        Some(MessageContent::Blocks(ref blocks)) if blocks.is_empty() => false,
        Some(_) => true,
    });
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::*;

    #[allow(clippy::needless_pass_by_value)]
    fn run_transform(input: Value) -> Value {
        let bytes = serde_json::to_vec(&input).unwrap();
        let config = TransformConfig::default();
        let tool_name_mapper =
            ToolNameMapper::new(config.tool_name_hash_len, config.tool_name_max_hash_len);
        let output = transform_body(&bytes, &config, &tool_name_mapper).unwrap();
        serde_json::from_slice(&output).unwrap()
    }

    #[test]
    fn transform_body_moves_non_core_system_text_and_obfuscates_tool_names() {
        let input = json!({
            "system": [{ "type": "text", "text": "OpenCode and opencode" }],
            "tools": [{ "name": "search" }],
            "messages": [
                { "role": "user", "content": [{ "type": "tool_use", "name": "lookup" }] }
            ]
        });

        let parsed = run_transform(input);

        assert_eq!(parsed["system"].as_array().unwrap().len(), 2);
        assert!(
            parsed["system"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with("x-anthropic-billing-header:")
        );

        assert_eq!(parsed["messages"][0]["content"][0]["type"], "text");
        assert_eq!(
            parsed["messages"][0]["content"][0]["text"],
            "OpenCode and opencode"
        );
        assert!(parsed["tools"][0]["name"]
            .as_str()
            .unwrap()
            .starts_with("t_"));
        assert!(parsed["messages"][0]["content"][1]["name"]
            .as_str()
            .unwrap()
            .starts_with("t_"));
    }

    #[test]
    fn transform_body_relocates_non_core_system_text_to_user_message() {
        let input = json!({
            "system": [{ "type": "text", "text": "Use opencode-claude-auth plugin instructions as-is." }],
            "messages": [{ "role": "user", "content": "hello" }]
        });

        let parsed = run_transform(input);

        assert_eq!(parsed["system"].as_array().unwrap().len(), 2);
        assert!(
            parsed["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("Use opencode-claude-auth plugin instructions as-is.")
        );
    }

    #[test]
    fn transform_body_relocates_url_path_system_text_to_user_message() {
        let input = json!({
            "system": [{ "type": "text", "text": "OpenCode docs: https://example.com/opencode/docs and path /var/opencode/bin" }],
            "messages": [{ "role": "user", "content": "hello" }]
        });

        let parsed = run_transform(input);

        assert_eq!(parsed["system"].as_array().unwrap().len(), 2);
        assert!(parsed["messages"][0]["content"].as_str().unwrap().contains(
            "OpenCode docs: https://example.com/opencode/docs and path /var/opencode/bin"
        ));
    }

    #[test]
    fn transform_body_injects_billing_header_with_computed_cch() {
        let input = json!({
            "system": [{ "type": "text", "text": "system prompt" }],
            "messages": [{ "role": "user", "content": "hey" }]
        });

        let parsed = run_transform(input);
        let billing = parsed["system"][0]["text"].as_str().unwrap();

        assert!(billing.starts_with("x-anthropic-billing-header:"));
        assert!(
            billing.contains("cch=fa690"),
            "Expected cch=fa690, got: {billing}"
        );
    }

    #[test]
    fn transform_body_billing_header_has_no_cache_control() {
        let input = json!({
            "system": [{ "type": "text", "text": "prompt", "cache_control": { "type": "ephemeral" } }],
            "messages": [{ "role": "user", "content": "test" }]
        });

        let parsed = run_transform(input);
        assert!(parsed["system"][0].get("cache_control").is_none());
    }

    #[test]
    fn transform_body_splits_identity_prefix_and_relocates_remainder() {
        let identity = "You are Claude Code, Anthropic's official CLI for Claude.";
        let input = json!({
            "system": [{ "type": "text", "text": format!("{identity}\nWorking directory: /home/test") }],
            "messages": [{ "role": "user", "content": "test" }]
        });

        let parsed = run_transform(input);

        assert!(
            parsed["system"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with("x-anthropic-billing-header:")
        );
        assert_eq!(parsed["system"][1]["text"], identity);
        assert_eq!(parsed["system"].as_array().unwrap().len(), 2);
        assert!(
            parsed["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("Working directory: /home/test")
        );
    }

    #[test]
    fn transform_body_preserves_identity_without_cache_control_and_relocates_remainder() {
        let identity = "You are Claude Code, Anthropic's official CLI for Claude.";
        let input = json!({
            "system": [{
                "type": "text",
                "text": format!("{identity}\nMore content here"),
                "cache_control": { "type": "ephemeral", "ttl": "1h" }
            }],
            "messages": [{ "role": "user", "content": "test" }]
        });

        let parsed = run_transform(input);

        assert!(parsed["system"][1].get("cache_control").is_none());
        assert_eq!(parsed["system"].as_array().unwrap().len(), 2);
        assert!(
            parsed["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("More content here")
        );
    }

    #[test]
    fn transform_body_does_not_split_identity_only_entry() {
        let identity = "You are Claude Code, Anthropic's official CLI for Claude.";
        let input = json!({
            "system": [{ "type": "text", "text": identity }],
            "messages": [{ "role": "user", "content": "test" }]
        });

        let parsed = run_transform(input);

        assert_eq!(parsed["system"].as_array().unwrap().len(), 2);
        assert_eq!(parsed["system"][1]["text"], identity);
    }

    #[test]
    fn transform_body_removes_duplicate_billing_headers_and_relocates_non_core_text() {
        let input = json!({
            "system": [
                { "type": "text", "text": "x-anthropic-billing-header: cc_version=old; cc_entrypoint=cli; cch=00000;" },
                { "type": "text", "text": "prompt" }
            ],
            "messages": [{ "role": "user", "content": "hey" }]
        });

        let parsed = run_transform(input);
        let billing_entries: Vec<&Value> = parsed["system"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|e| {
                e["text"]
                    .as_str()
                    .unwrap_or("")
                    .starts_with("x-anthropic-billing-header:")
            })
            .collect();

        assert_eq!(billing_entries.len(), 1);
        assert!(
            billing_entries[0]["text"]
                .as_str()
                .unwrap()
                .contains("cch=fa690")
        );
        assert!(
            parsed["messages"][0]["content"]
                .as_str()
                .unwrap()
                .contains("prompt")
        );
    }

    #[test]
    fn transform_body_relocates_multiple_non_core_system_entries_to_user_blocks() {
        let identity = "You are Claude Code, Anthropic's official CLI for Claude.";
        let input = json!({
            "system": [
                { "type": "text", "text": identity },
                { "type": "text", "text": "Custom instructions block A" },
                { "type": "text", "text": "Custom instructions block B" }
            ],
            "messages": [{
                "role": "user",
                "content": [{ "type": "text", "text": "hello" }]
            }]
        });

        let parsed = run_transform(input);

        assert_eq!(parsed["system"].as_array().unwrap().len(), 2);
        assert!(
            parsed["system"][0]["text"]
                .as_str()
                .unwrap()
                .starts_with("x-anthropic-billing-header:")
        );
        assert_eq!(parsed["system"][1]["text"], identity);

        assert_eq!(parsed["messages"][0]["content"][0]["type"], "text");
        let prepended = parsed["messages"][0]["content"][0]["text"]
            .as_str()
            .unwrap();
        assert!(prepended.contains("Custom instructions block A"));
        assert!(prepended.contains("Custom instructions block B"));
        assert_eq!(parsed["messages"][0]["content"][1]["text"], "hello");
    }

    #[test]
    fn transform_body_keeps_system_when_no_messages_exist() {
        let input = json!({
            "system": [{ "type": "text", "text": "Some instructions" }],
            "messages": []
        });

        let parsed = run_transform(input);

        assert!(parsed["system"].as_array().unwrap().len() >= 2);
    }

    #[test]
    fn transform_body_strips_output_config_effort_for_haiku() {
        let input = json!({
            "model": "claude-haiku-4-5-20251001",
            "output_config": { "effort": "high" },
            "messages": [{ "role": "user", "content": "test" }]
        });

        let parsed = run_transform(input);
        assert!(parsed.get("output_config").is_none());
    }

    #[test]
    fn transform_body_strips_effort_but_keeps_other_output_config_for_haiku() {
        let input = json!({
            "model": "claude-haiku-4-5-20251001",
            "output_config": { "effort": "high", "max_tokens": 1024 },
            "messages": [{ "role": "user", "content": "test" }]
        });

        let parsed = run_transform(input);

        assert!(parsed.get("output_config").is_some());
        assert_eq!(parsed["output_config"]["max_tokens"], 1024);
        assert!(parsed["output_config"].get("effort").is_none());
    }

    #[test]
    fn transform_body_strips_thinking_effort_but_preserves_other_fields_for_haiku() {
        let input = json!({
            "model": "claude-haiku-4-5-20251001",
            "thinking": { "type": "enabled", "effort": "high" },
            "messages": [{ "role": "user", "content": "test" }]
        });

        let parsed = run_transform(input);

        assert!(parsed.get("thinking").is_some());
        assert!(parsed["thinking"].get("effort").is_none());
        assert_eq!(parsed["thinking"]["type"], "enabled");
    }

    #[test]
    fn transform_body_removes_thinking_when_effort_only_for_haiku() {
        let input = json!({
            "model": "claude-haiku-4-5-20251001",
            "thinking": { "effort": "high" },
            "messages": [{ "role": "user", "content": "test" }]
        });

        let parsed = run_transform(input);
        assert!(parsed.get("thinking").is_none());
    }

    #[test]
    fn transform_body_preserves_thinking_for_haiku_when_effort_absent() {
        let input = json!({
            "model": "claude-haiku-4-5-20251001",
            "thinking": { "type": "enabled" },
            "messages": [{ "role": "user", "content": "test" }]
        });

        let parsed = run_transform(input);
        assert_eq!(parsed["thinking"], json!({ "type": "enabled" }));
    }

    #[test]
    fn transform_body_preserves_effort_for_non_haiku_models() {
        let input = json!({
            "model": "claude-opus-4-6",
            "output_config": { "effort": "high" },
            "thinking": { "type": "enabled", "effort": "high" },
            "messages": [{ "role": "user", "content": "test" }]
        });

        let parsed = run_transform(input);

        assert_eq!(parsed["output_config"]["effort"], "high");
        assert_eq!(parsed["thinking"]["effort"], "high");
    }

    #[test]
    fn transform_body_handles_haiku_without_effort_related_fields() {
        let input = json!({
            "model": "claude-haiku-4-5",
            "messages": [{ "role": "user", "content": "test" }]
        });

        let parsed = run_transform(input);

        assert!(parsed.get("output_config").is_none());
        assert!(parsed.get("thinking").is_none());
    }

    #[test]
    fn transform_body_prefixes_tool_choice_name() {
        let input = json!({
            "tools": [{ "name": "get_weather", "description": "Get weather" }],
            "tool_choice": { "type": "tool", "name": "get_weather" },
            "messages": [{ "role": "user", "content": "test" }]
        });

        let parsed = run_transform(input);

        assert_eq!(parsed["tool_choice"]["type"], "tool");
        assert!(parsed["tool_choice"]["name"]
            .as_str()
            .unwrap()
            .starts_with("t_"));
    }

    #[test]
    fn transform_body_preserves_tool_choice_type_any_without_name() {
        let input = json!({
            "tools": [{ "name": "search" }],
            "tool_choice": { "type": "any" },
            "messages": [{ "role": "user", "content": "test" }]
        });

        let parsed = run_transform(input);

        assert_eq!(parsed["tool_choice"]["type"], "any");
        assert!(
            parsed["tool_choice"].get("name").is_none(),
            "tool_choice should not gain a name field"
        );
    }

    #[test]
    fn transform_body_preserves_tool_choice_type_auto_without_name() {
        let input = json!({
            "tools": [{ "name": "search" }],
            "tool_choice": { "type": "auto" },
            "messages": [{ "role": "user", "content": "test" }]
        });

        let parsed = run_transform(input);

        assert_eq!(parsed["tool_choice"]["type"], "auto");
        assert!(
            parsed["tool_choice"].get("name").is_none(),
            "tool_choice should not gain a name field"
        );
    }

    #[test]
    fn transform_body_omits_tool_choice_when_absent() {
        let input = json!({
            "tools": [{ "name": "search" }],
            "messages": [{ "role": "user", "content": "test" }]
        });

        let parsed = run_transform(input);

        assert!(
            parsed.get("tool_choice").is_none(),
            "tool_choice should remain absent when not provided"
        );
    }

    #[test]
    fn transform_body_prefixes_tool_choice_name_alongside_tools() {
        let input = json!({
            "tools": [
                { "name": "search" },
                { "name": "analyze" }
            ],
            "tool_choice": { "type": "tool", "name": "analyze" },
            "messages": [{ "role": "user", "content": "test" }]
        });

        let parsed = run_transform(input);

        assert!(parsed["tools"][0]["name"].as_str().unwrap().starts_with("t_"));
        assert!(parsed["tools"][1]["name"].as_str().unwrap().starts_with("t_"));
        assert_eq!(parsed["tool_choice"]["type"], "tool");
        assert_eq!(parsed["tool_choice"]["name"], parsed["tools"][1]["name"]);
    }

    #[test]
    fn transform_body_preserves_tool_choice_disable_parallel_tool_use() {
        let input = json!({
            "tools": [{ "name": "analyze" }],
            "tool_choice": {
                "type": "tool",
                "name": "analyze",
                "disable_parallel_tool_use": true
            },
            "messages": [{ "role": "user", "content": "test" }]
        });

        let parsed = run_transform(input);

        assert_eq!(parsed["tool_choice"]["type"], "tool");
        assert_eq!(parsed["tool_choice"]["name"], parsed["tools"][0]["name"]);
        assert_eq!(parsed["tool_choice"]["disable_parallel_tool_use"], true);
    }
}
