//! Common controls whose wire shape differs between Chat and Responses.

use super::{BedrockJsonSchema, BedrockToolChoice};
use crate::provider::ProviderError;
use crate::types::ToolDefinition;
use serde_json::{json, Value};

fn invalid(message: &str) -> ProviderError {
    ProviderError::Configuration(message.into())
}

pub(super) fn validate_schema(schema: &BedrockJsonSchema) -> Result<(), ProviderError> {
    if schema.name.is_empty()
        || schema.name.len() > 64
        || !schema
            .name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
        || !schema.schema.is_object()
    {
        return Err(invalid(
            "Structured output requires a schema object and a short alphanumeric name",
        ));
    }
    Ok(())
}

pub(super) fn apply_tool_choice(
    body: &mut Value,
    tools: &[ToolDefinition],
    choice: Option<&BedrockToolChoice>,
    responses: bool,
) -> Result<(), ProviderError> {
    let Some(choice) = choice else { return Ok(()) };
    if !matches!(choice, BedrockToolChoice::None) && tools.is_empty() {
        return Err(invalid("Tool choice requires tool definitions"));
    }
    body["tool_choice"] = match choice {
        BedrockToolChoice::Auto => json!("auto"),
        BedrockToolChoice::None => json!("none"),
        BedrockToolChoice::Any => json!("required"),
        BedrockToolChoice::Tool(name) => {
            if !tools.iter().any(|tool| &tool.name == name) {
                return Err(invalid(
                    "The selected tool must exist in this request's tool definitions",
                ));
            }
            if responses {
                json!({"type":"function", "name":name})
            } else {
                json!({"type":"function", "function":{"name":name}})
            }
        }
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_choice_shapes_and_missing_tools_are_explicit() {
        let tools = vec![ToolDefinition {
            name: "count".into(),
            description: "fixture".into(),
            input_schema: json!({"type":"object"}),
        }];
        for responses in [false, true] {
            let mut body = json!({});
            apply_tool_choice(
                &mut body,
                &tools,
                Some(&BedrockToolChoice::Tool("count".into())),
                responses,
            )
            .unwrap();
            assert_eq!(
                body["tool_choice"],
                if responses {
                    json!({"type":"function","name":"count"})
                } else {
                    json!({"type":"function","function":{"name":"count"}})
                }
            );
            assert!(
                apply_tool_choice(&mut body, &[], Some(&BedrockToolChoice::Any), responses)
                    .is_err()
            );
            assert!(apply_tool_choice(
                &mut body,
                &tools,
                Some(&BedrockToolChoice::Tool("absent".into())),
                responses
            )
            .is_err());
            apply_tool_choice(&mut body, &[], Some(&BedrockToolChoice::None), responses).unwrap();
            assert_eq!(body["tool_choice"], "none");
        }
    }
}
