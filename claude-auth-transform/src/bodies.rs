use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageBody {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    #[serde(
        default,
        skip_serializing_if = "Vec::is_empty",
        deserialize_with = "deserialize_system"
    )]
    pub system: Vec<SystemEntry>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub thinking: HashMap<String, Value>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub output_config: HashMap<String, Value>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tools: Vec<Tool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool_choice: Option<ToolChoice>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub messages: Vec<Message>,
    #[serde(default, flatten)]
    pub extra: HashMap<String, Value>,
}

fn deserialize_system<'de, D>(deserializer: D) -> Result<Vec<SystemEntry>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    use std::fmt;

    use serde::de::{self, Visitor};

    struct SystemVisitor;

    impl<'de> Visitor<'de> for SystemVisitor {
        type Value = Vec<SystemEntry>;

        fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
            formatter.write_str("a string or array of system content blocks")
        }

        fn visit_str<E: de::Error>(self, value: &str) -> Result<Self::Value, E> {
            Ok(vec![SystemEntry {
                r#type: Some("text".to_owned()),
                text: Some(value.to_owned()),
                extra: HashMap::new(),
            }])
        }

        fn visit_string<E: de::Error>(self, value: String) -> Result<Self::Value, E> {
            Ok(vec![SystemEntry {
                r#type: Some("text".to_owned()),
                text: Some(value),
                extra: HashMap::new(),
            }])
        }

        fn visit_none<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }

        fn visit_unit<E: de::Error>(self) -> Result<Self::Value, E> {
            Ok(Vec::new())
        }

        fn visit_seq<A>(self, seq: A) -> Result<Self::Value, A::Error>
        where
            A: de::SeqAccess<'de>,
        {
            Vec::<SystemEntry>::deserialize(de::value::SeqAccessDeserializer::new(seq))
        }
    }

    deserializer.deserialize_any(SystemVisitor)
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Tool {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(default, flatten)]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ToolChoice {
    #[serde(rename = "type", default, skip_serializing_if = "String::is_empty")]
    pub r#type: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    #[serde(flatten, default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SystemEntry {
    #[serde(rename = "type", default, skip_serializing_if = "Option::is_none")]
    pub r#type: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    #[serde(default, flatten)]
    pub extra: HashMap<String, Value>,
}

pub type ContentBlock = SystemEntry;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum MessageContent {
    Text(String),
    Blocks(Vec<ContentBlock>),
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Message {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub content: Option<MessageContent>,
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub extra: HashMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn deserializes_string_form_system_into_single_text_entry() {
        let json = r#"{"system":"You are a helpful assistant."}"#;
        let body: MessageBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.system.len(), 1);
        assert_eq!(body.system[0].r#type.as_deref(), Some("text"));
        assert_eq!(
            body.system[0].text.as_deref(),
            Some("You are a helpful assistant.")
        );
    }

    #[test]
    fn deserializes_array_form_system_unchanged() {
        let json =
            r#"{"system":[{"type":"text","text":"hi","cache_control":{"type":"ephemeral"}}]}"#;
        let body: MessageBody = serde_json::from_str(json).unwrap();
        assert_eq!(body.system.len(), 1);
        assert_eq!(body.system[0].text.as_deref(), Some("hi"));
        assert!(body.system[0].extra.contains_key("cache_control"));
    }

    #[test]
    fn deserializes_missing_system_as_empty_vec() {
        let body: MessageBody = serde_json::from_str(r"{}").unwrap();
        assert!(body.system.is_empty());
    }

    #[test]
    fn deserializes_null_system_as_empty_vec() {
        let body: MessageBody = serde_json::from_str(r#"{"system":null}"#).unwrap();
        assert!(body.system.is_empty());
    }

    #[test]
    fn deserializes_empty_string_system_as_single_empty_text_entry() {
        let body: MessageBody = serde_json::from_str(r#"{"system":""}"#).unwrap();
        assert_eq!(body.system.len(), 1);
        assert_eq!(body.system[0].text.as_deref(), Some(""));
    }
}
