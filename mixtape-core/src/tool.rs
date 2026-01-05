use schemars::JsonSchema;
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Image formats supported for tool results
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ImageFormat {
    Png,
    Jpeg,
    Gif,
    Webp,
}

/// Document formats supported for tool results
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DocumentFormat {
    Pdf,
    Csv,
    Doc,
    Docx,
    Html,
    Md,
    Txt,
    Xls,
    Xlsx,
}

/// Result types that tools can return.
///
/// Tools can return different content types depending on their purpose.
/// All providers support Text and Json. Image and Document support varies by provider
/// (Bedrock supports all types; future providers may fall back to text descriptions).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ToolResult {
    /// Plain text response
    Text(String),

    /// Structured JSON data - use for complex responses
    Json(Value),

    /// Image data - supported by Bedrock (Claude, Nova models)
    Image {
        format: ImageFormat,
        /// Raw image bytes (not base64 encoded)
        data: Vec<u8>,
    },

    /// Document data - supported by Bedrock (Claude, Nova models)
    Document {
        format: DocumentFormat,
        /// Raw document bytes
        data: Vec<u8>,
        /// Optional document name/filename
        name: Option<String>,
    },
}

impl ToolResult {
    /// Create a JSON result from any serializable type
    pub fn json<T: Serialize>(value: T) -> Result<Self, serde_json::Error> {
        Ok(Self::Json(serde_json::to_value(value)?))
    }

    /// Create a text result from a string
    pub fn text(s: impl Into<String>) -> Self {
        Self::Text(s.into())
    }

    /// Create an image result from raw bytes
    pub fn image(format: ImageFormat, data: Vec<u8>) -> Self {
        Self::Image { format, data }
    }

    /// Create a document result from raw bytes
    pub fn document(format: DocumentFormat, data: Vec<u8>) -> Self {
        Self::Document {
            format,
            data,
            name: None,
        }
    }

    /// Create a document result with a filename
    pub fn document_with_name(
        format: DocumentFormat,
        data: Vec<u8>,
        name: impl Into<String>,
    ) -> Self {
        Self::Document {
            format,
            data,
            name: Some(name.into()),
        }
    }

    /// Get the text content if this is a Text variant, or convert to string description
    pub fn as_text(&self) -> String {
        match self {
            ToolResult::Text(s) => s.clone(),
            ToolResult::Json(v) => v.to_string(),
            ToolResult::Image { format, data } => {
                format!("[Image: {:?}, {} bytes]", format, data.len())
            }
            ToolResult::Document { format, data, name } => {
                let name_str = name.as_deref().unwrap_or("unnamed");
                format!(
                    "[Document: {:?}, {}, {} bytes]",
                    format,
                    name_str,
                    data.len()
                )
            }
        }
    }

    /// Get a reference to the text content if this is a Text variant
    pub fn as_str(&self) -> Option<&str> {
        match self {
            ToolResult::Text(s) => Some(s),
            _ => None,
        }
    }
}

/// Convert strings directly to ToolResult::Text
impl From<String> for ToolResult {
    fn from(s: String) -> Self {
        Self::Text(s)
    }
}

impl From<&str> for ToolResult {
    fn from(s: &str) -> Self {
        Self::Text(s.to_string())
    }
}

/// Errors that can occur during tool execution
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    Serialization(#[from] serde_json::Error),

    #[error("Path validation failed: {0}")]
    PathValidation(String),

    #[error("{0}")]
    Custom(String),
}

impl From<String> for ToolError {
    fn from(s: String) -> Self {
        Self::Custom(s)
    }
}

impl From<&str> for ToolError {
    fn from(s: &str) -> Self {
        Self::Custom(s.to_string())
    }
}

/// Trait for implementing tools that can be used by AI agents.
///
/// Tools define an input type with `#[derive(Deserialize, JsonSchema)]` to automatically
/// generate JSON schemas from Rust types, providing excellent developer experience.
///
/// # Async Tools Example
///
/// ```rust
/// use mixtape_core::{Tool, ToolResult, ToolError};
/// use schemars::JsonSchema;
/// use serde::Deserialize;
/// use std::time::Duration;
///
/// #[derive(Deserialize, JsonSchema)]
/// struct DelayInput {
///     /// Duration in milliseconds
///     ms: u64,
/// }
///
/// struct DelayTool;
///
/// impl Tool for DelayTool {
///     type Input = DelayInput;
///
///     fn name(&self) -> &str { "delay" }
///     fn description(&self) -> &str { "Wait for a duration" }
///
///     fn execute(&self, input: Self::Input) -> impl std::future::Future<Output = Result<ToolResult, ToolError>> + Send {
///         async move {
///             // Async operations work naturally
///             tokio::time::sleep(Duration::from_millis(input.ms)).await;
///             Ok(format!("Waited {}ms", input.ms).into())  // Converts to ToolResult::Text
///         }
///     }
/// }
/// ```
///
/// # Returning JSON Data
///
/// Tools can return structured JSON data:
///
/// ```rust
/// use mixtape_core::{Tool, ToolResult, ToolError};
/// use schemars::JsonSchema;
/// use serde::{Deserialize, Serialize};
///
/// #[derive(Deserialize, JsonSchema)]
/// struct CalculateInput {
///     a: i32,
///     b: i32,
/// }
///
/// #[derive(Serialize)]
/// struct CalculateOutput {
///     sum: i32,
///     product: i32,
/// }
///
/// struct CalculateTool;
///
/// impl Tool for CalculateTool {
///     type Input = CalculateInput;
///
///     fn name(&self) -> &str { "calculate" }
///     fn description(&self) -> &str { "Perform calculations" }
///
///     fn execute(&self, input: Self::Input) -> impl std::future::Future<Output = Result<ToolResult, ToolError>> + Send {
///         async move {
///             let output = CalculateOutput {
///                 sum: input.a + input.b,
///                 product: input.a * input.b,
///             };
///             ToolResult::json(output).map_err(Into::into)
///         }
///     }
/// }
/// ```
pub trait Tool: Send + Sync {
    /// The input type for this tool. Must implement `Deserialize` and `JsonSchema`.
    type Input: DeserializeOwned + JsonSchema;

    /// The name of the tool (e.g., "read_file", "calculator")
    fn name(&self) -> &str;

    /// A description of what the tool does
    fn description(&self) -> &str;

    /// Execute the tool with typed input
    fn execute(
        &self,
        input: Self::Input,
    ) -> impl std::future::Future<Output = Result<ToolResult, ToolError>> + Send;

    /// Get the JSON schema for this tool's input.
    ///
    /// This is automatically implemented using the `JsonSchema` derive on `Input`.
    /// The schema is generated at runtime from the type definition.
    fn input_schema(&self) -> Value {
        let schema = schemars::schema_for!(Self::Input);
        serde_json::to_value(schema).expect("Failed to serialize schema")
    }

    // ========================================================================
    // Formatting methods - override these for custom tool presentation
    // ========================================================================

    /// Format tool input as plain text (for JIRA, logs, copy/paste).
    ///
    /// Default implementation shows tool name and parameters with truncation.
    fn format_input_plain(&self, params: &Value) -> String {
        format_params_plain(self.name(), params)
    }

    /// Format tool input with ANSI colors (for terminal display).
    ///
    /// Default implementation shows tool name (bold) and parameters with colors.
    fn format_input_ansi(&self, params: &Value) -> String {
        format_params_ansi(self.name(), params)
    }

    /// Format tool input as Markdown (for docs, GitHub, rendered UIs).
    ///
    /// Default implementation shows tool name and parameters in markdown format.
    fn format_input_markdown(&self, params: &Value) -> String {
        format_params_markdown(self.name(), params)
    }

    /// Format tool output as plain text.
    ///
    /// Default implementation shows result text with truncation.
    fn format_output_plain(&self, result: &ToolResult) -> String {
        format_result_plain(result)
    }

    /// Format tool output with ANSI colors.
    ///
    /// Default implementation shows result with success indicator and truncation.
    fn format_output_ansi(&self, result: &ToolResult) -> String {
        format_result_ansi(result)
    }

    /// Format tool output as Markdown.
    ///
    /// Default implementation shows result in a code block with truncation.
    fn format_output_markdown(&self, result: &ToolResult) -> String {
        format_result_markdown(result)
    }
}

/// Object-safe trait for dynamic tool dispatch (used internally by the agent).
///
/// Users should implement `Tool` instead and use `box_tool()` to convert.
pub trait DynTool: Send + Sync {
    fn name(&self) -> &str;
    fn description(&self) -> &str;
    fn input_schema(&self) -> Value;
    fn execute_raw(
        &self,
        input: Value,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolResult, ToolError>> + Send + '_>,
    >;

    // Formatting methods
    fn format_input_plain(&self, params: &Value) -> String;
    fn format_input_ansi(&self, params: &Value) -> String;
    fn format_input_markdown(&self, params: &Value) -> String;
    fn format_output_plain(&self, result: &ToolResult) -> String;
    fn format_output_ansi(&self, result: &ToolResult) -> String;
    fn format_output_markdown(&self, result: &ToolResult) -> String;
}

/// Convert a `Tool` into a type-erased `Box<dyn DynTool>` for storage in collections.
pub fn box_tool<T: Tool + 'static>(tool: T) -> Box<dyn DynTool> {
    Box::new(ToolWrapper(tool))
}

/// Create a `Vec<Box<dyn DynTool>>` from heterogeneous tool types.
///
/// This macro boxes each tool and collects them into a vector that can be
/// passed to [`crate::AgentBuilder::add_tools()`].
///
/// # Example
///
/// ```ignore
/// use mixtape_core::{Agent, box_tools, ClaudeSonnet4};
///
/// let agent = Agent::builder()
///     .bedrock(ClaudeSonnet4)
///     .add_tools(box_tools![Calculator, WeatherLookup, FileReader])
///     .build()
///     .await?;
/// ```
///
/// This is equivalent to:
///
/// ```ignore
/// .add_tool(Calculator)
/// .add_tool(WeatherLookup)
/// .add_tool(FileReader)
/// ```
#[macro_export]
macro_rules! box_tools {
    ($($tool:expr),* $(,)?) => {
        vec![$($crate::tool::box_tool($tool)),*]
    };
}

/// Internal wrapper that implements DynTool for any Tool
struct ToolWrapper<T>(T);

impl<T: Tool + 'static> DynTool for ToolWrapper<T> {
    fn name(&self) -> &str {
        self.0.name()
    }

    fn description(&self) -> &str {
        self.0.description()
    }

    fn input_schema(&self) -> Value {
        self.0.input_schema()
    }

    fn execute_raw(
        &self,
        input: Value,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<ToolResult, ToolError>> + Send + '_>,
    > {
        Box::pin(async move {
            let typed_input: T::Input = serde_json::from_value(input)
                .map_err(|e| ToolError::Custom(format!("Failed to deserialize input: {}", e)))?;

            self.0.execute(typed_input).await
        })
    }

    fn format_input_plain(&self, params: &Value) -> String {
        self.0.format_input_plain(params)
    }

    fn format_input_ansi(&self, params: &Value) -> String {
        self.0.format_input_ansi(params)
    }

    fn format_input_markdown(&self, params: &Value) -> String {
        self.0.format_input_markdown(params)
    }

    fn format_output_plain(&self, result: &ToolResult) -> String {
        self.0.format_output_plain(result)
    }

    fn format_output_ansi(&self, result: &ToolResult) -> String {
        self.0.format_output_ansi(result)
    }

    fn format_output_markdown(&self, result: &ToolResult) -> String {
        self.0.format_output_markdown(result)
    }
}

// ============================================================================
// Default formatting helpers
// ============================================================================

const MAX_PARAMS: usize = 10;
const MAX_VALUE_LEN: usize = 80;
const MAX_OUTPUT_LINES: usize = 12;

/// Format a JSON value for display, with truncation
fn format_value_preview(value: &Value) -> String {
    match value {
        Value::String(s) => {
            if s.len() > MAX_VALUE_LEN {
                format!("\"{}…\"", &s[..MAX_VALUE_LEN])
            } else {
                format!("\"{}\"", s)
            }
        }
        Value::Array(arr) => format!("[{} items]", arr.len()),
        Value::Object(obj) => format!("{{{} keys}}", obj.len()),
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
    }
}

/// Format tool parameters as plain text
pub fn format_params_plain(tool_name: &str, params: &Value) -> String {
    let mut output = tool_name.to_string();

    if let Some(obj) = params.as_object() {
        for (key, value) in obj.iter().take(MAX_PARAMS) {
            output.push_str(&format!("\n  {}: {}", key, format_value_preview(value)));
        }
        if obj.len() > MAX_PARAMS {
            output.push_str(&format!("\n  … +{} more", obj.len() - MAX_PARAMS));
        }
    }

    output
}

/// Format tool parameters with ANSI colors
pub fn format_params_ansi(tool_name: &str, params: &Value) -> String {
    // Bold tool name
    let mut output = format!("\x1b[1m{}\x1b[0m", tool_name);

    if let Some(obj) = params.as_object() {
        for (key, value) in obj.iter().take(MAX_PARAMS) {
            // Dim key, normal value
            output.push_str(&format!(
                "\n  \x1b[2m{}:\x1b[0m {}",
                key,
                format_value_preview(value)
            ));
        }
        if obj.len() > MAX_PARAMS {
            output.push_str(&format!(
                "\n  \x1b[2m… +{} more\x1b[0m",
                obj.len() - MAX_PARAMS
            ));
        }
    }

    output
}

/// Format tool parameters as Markdown
pub fn format_params_markdown(tool_name: &str, params: &Value) -> String {
    let mut output = format!("**{}**", tool_name);

    if let Some(obj) = params.as_object() {
        for (key, value) in obj.iter().take(MAX_PARAMS) {
            output.push_str(&format!("\n- `{}`: {}", key, format_value_preview(value)));
        }
        if obj.len() > MAX_PARAMS {
            output.push_str(&format!("\n- *… +{} more*", obj.len() - MAX_PARAMS));
        }
    }

    output
}

/// Get text representation of a ToolResult
fn result_to_text(result: &ToolResult) -> String {
    match result {
        ToolResult::Text(s) => s.clone(),
        ToolResult::Json(v) => format_json_truncated(v),
        ToolResult::Image { format, data } => {
            format!("[Image: {:?}, {} bytes]", format, data.len())
        }
        ToolResult::Document { format, data, name } => {
            let name_str = name.as_deref().unwrap_or("unnamed");
            format!(
                "[Document: {:?}, {}, {} bytes]",
                format,
                name_str,
                data.len()
            )
        }
    }
}

/// Format JSON with truncated string values and limited object keys
fn format_json_truncated(value: &Value) -> String {
    format_json_truncated_inner(value, 0)
}

fn format_json_truncated_inner(value: &Value, depth: usize) -> String {
    let indent = "  ".repeat(depth);
    let child_indent = "  ".repeat(depth + 1);

    match value {
        Value::String(s) => {
            if s.len() > MAX_VALUE_LEN {
                format!("\"{}…\"", &s[..MAX_VALUE_LEN])
            } else {
                format!("\"{}\"", s)
            }
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                "[]".to_string()
            } else if arr.len() > MAX_PARAMS {
                format!("[{} items]", arr.len())
            } else {
                let items: Vec<String> = arr
                    .iter()
                    .take(MAX_PARAMS)
                    .map(|v| {
                        format!(
                            "{}{}",
                            child_indent,
                            format_json_truncated_inner(v, depth + 1)
                        )
                    })
                    .collect();
                format!("[\n{}\n{}]", items.join(",\n"), indent)
            }
        }
        Value::Object(obj) => {
            if obj.is_empty() {
                "{}".to_string()
            } else {
                let mut items: Vec<String> = obj
                    .iter()
                    .take(MAX_PARAMS)
                    .map(|(k, v)| {
                        format!(
                            "{}\"{}\": {}",
                            child_indent,
                            k,
                            format_json_truncated_inner(v, depth + 1)
                        )
                    })
                    .collect();
                if obj.len() > MAX_PARAMS {
                    items.push(format!(
                        "{}… +{} more",
                        child_indent,
                        obj.len() - MAX_PARAMS
                    ));
                }
                format!("{{\n{}\n{}}}", items.join(",\n"), indent)
            }
        }
        Value::Null => "null".to_string(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => n.to_string(),
    }
}

/// Truncate text to max lines, returning (truncated_text, remaining_lines)
fn truncate_lines(text: &str, max_lines: usize) -> (String, usize) {
    let lines: Vec<&str> = text.lines().collect();
    if lines.len() <= max_lines {
        (text.to_string(), 0)
    } else {
        let truncated = lines[..max_lines].join("\n");
        (truncated, lines.len() - max_lines)
    }
}

/// Format tool result as plain text
pub fn format_result_plain(result: &ToolResult) -> String {
    let text = result_to_text(result);
    let (truncated, remaining) = truncate_lines(&text, MAX_OUTPUT_LINES);

    if remaining > 0 {
        format!("{}\n… +{} more lines", truncated, remaining)
    } else {
        truncated
    }
}

/// Format tool result with ANSI colors
pub fn format_result_ansi(result: &ToolResult) -> String {
    let text = result_to_text(result);
    let (truncated, remaining) = truncate_lines(&text, MAX_OUTPUT_LINES);

    if remaining > 0 {
        format!(
            "\x1b[32m✓\x1b[0m\n{}\n\x1b[2m… +{} more lines\x1b[0m",
            truncated, remaining
        )
    } else {
        format!("\x1b[32m✓\x1b[0m\n{}", truncated)
    }
}

/// Format tool result as Markdown
pub fn format_result_markdown(result: &ToolResult) -> String {
    let text = result_to_text(result);
    let (truncated, remaining) = truncate_lines(&text, MAX_OUTPUT_LINES);

    let mut output = String::from("```\n");
    output.push_str(&truncated);
    output.push_str("\n```");

    if remaining > 0 {
        output.push_str(&format!("\n*… +{} more lines*", remaining));
    }

    output
}

#[cfg(test)]
mod tests {
    use super::*;

    // ===== format_value_preview tests =====

    #[test]
    fn test_format_value_preview_string_short() {
        let value = serde_json::json!("hello");
        assert_eq!(format_value_preview(&value), "\"hello\"");
    }

    #[test]
    fn test_format_value_preview_string_long() {
        let long_string = "x".repeat(100);
        let value = serde_json::json!(long_string);
        let preview = format_value_preview(&value);

        // Should be truncated to MAX_VALUE_LEN (80) + quotes + ellipsis
        assert!(preview.len() < 100);
        assert!(preview.ends_with("…\""));
    }

    #[test]
    fn test_format_value_preview_array() {
        let value = serde_json::json!([1, 2, 3, 4, 5]);
        assert_eq!(format_value_preview(&value), "[5 items]");
    }

    #[test]
    fn test_format_value_preview_object() {
        let value = serde_json::json!({"a": 1, "b": 2});
        assert_eq!(format_value_preview(&value), "{2 keys}");
    }

    #[test]
    fn test_format_value_preview_null() {
        let value = serde_json::json!(null);
        assert_eq!(format_value_preview(&value), "null");
    }

    #[test]
    fn test_format_value_preview_bool() {
        assert_eq!(format_value_preview(&serde_json::json!(true)), "true");
        assert_eq!(format_value_preview(&serde_json::json!(false)), "false");
    }

    #[test]
    fn test_format_value_preview_number() {
        assert_eq!(format_value_preview(&serde_json::json!(42)), "42");
        assert_eq!(format_value_preview(&serde_json::json!(1.5)), "1.5");
    }

    // ===== truncate_lines tests =====

    #[test]
    fn test_truncate_lines_no_truncation() {
        let text = "line1\nline2\nline3";
        let (result, remaining) = truncate_lines(text, 5);
        assert_eq!(result, text);
        assert_eq!(remaining, 0);
    }

    #[test]
    fn test_truncate_lines_with_truncation() {
        let text = "line1\nline2\nline3\nline4\nline5";
        let (result, remaining) = truncate_lines(text, 3);
        assert_eq!(result, "line1\nline2\nline3");
        assert_eq!(remaining, 2);
    }

    #[test]
    fn test_truncate_lines_exact_limit() {
        let text = "line1\nline2\nline3";
        let (result, remaining) = truncate_lines(text, 3);
        assert_eq!(result, text);
        assert_eq!(remaining, 0);
    }

    // ===== format_params tests =====

    #[test]
    fn test_format_params_plain_simple() {
        let params = serde_json::json!({"path": "/tmp/test.txt"});
        let output = format_params_plain("read_file", &params);

        assert!(output.starts_with("read_file"));
        assert!(output.contains("path:"));
        assert!(output.contains("/tmp/test.txt"));
    }

    #[test]
    fn test_format_params_plain_many_params() {
        // More than MAX_PARAMS (10) parameters
        let mut obj = serde_json::Map::new();
        for i in 0..15 {
            obj.insert(format!("key{}", i), serde_json::json!(i));
        }
        let params = serde_json::Value::Object(obj);
        let output = format_params_plain("test_tool", &params);

        assert!(output.contains("… +"));
        assert!(output.contains("more"));
    }

    #[test]
    fn test_format_params_ansi_has_codes() {
        let params = serde_json::json!({"name": "test"});
        let output = format_params_ansi("my_tool", &params);

        // Should contain ANSI escape codes
        assert!(output.contains("\x1b["));
        // Should contain tool name
        assert!(output.contains("my_tool"));
    }

    #[test]
    fn test_format_params_markdown_format() {
        let params = serde_json::json!({"file": "test.rs"});
        let output = format_params_markdown("edit", &params);

        // Should have bold tool name
        assert!(output.starts_with("**edit**"));
        // Should have markdown list items
        assert!(output.contains("- `file`:"));
    }

    // ===== format_result tests =====

    #[test]
    fn test_format_result_plain_short() {
        let result = ToolResult::Text("Success!".to_string());
        let output = format_result_plain(&result);
        assert_eq!(output, "Success!");
    }

    #[test]
    fn test_format_result_plain_truncated() {
        // More than MAX_OUTPUT_LINES (12) lines
        let long_text = (0..20)
            .map(|i| format!("line {}", i))
            .collect::<Vec<_>>()
            .join("\n");
        let result = ToolResult::Text(long_text);
        let output = format_result_plain(&result);

        assert!(output.contains("… +"));
        assert!(output.contains("more lines"));
    }

    #[test]
    fn test_format_result_ansi_success_marker() {
        let result = ToolResult::Text("Done".to_string());
        let output = format_result_ansi(&result);

        // Should have green checkmark
        assert!(output.contains("\x1b[32m✓\x1b[0m"));
    }

    #[test]
    fn test_format_result_markdown_code_block() {
        let result = ToolResult::Text("code here".to_string());
        let output = format_result_markdown(&result);

        assert!(output.starts_with("```\n"));
        assert!(output.contains("code here"));
        assert!(output.contains("\n```"));
    }

    #[test]
    fn test_format_result_json() {
        let result = ToolResult::Json(serde_json::json!({"status": "ok"}));
        let output = format_result_plain(&result);

        // JSON should be pretty-printed
        assert!(output.contains("status"));
        assert!(output.contains("ok"));
    }

    #[test]
    fn test_format_result_image() {
        let result = ToolResult::Image {
            format: ImageFormat::Png,
            data: vec![0u8; 1000],
        };
        let output = format_result_plain(&result);

        assert!(output.contains("Image"));
        assert!(output.contains("Png"));
        assert!(output.contains("1000 bytes"));
    }

    #[test]
    fn test_format_result_document() {
        let result = ToolResult::Document {
            format: DocumentFormat::Pdf,
            data: vec![0u8; 500],
            name: Some("report.pdf".to_string()),
        };
        let output = format_result_plain(&result);

        assert!(output.contains("Document"));
        assert!(output.contains("Pdf"));
        assert!(output.contains("report.pdf"));
        assert!(output.contains("500 bytes"));
    }

    #[test]
    fn test_format_result_document_unnamed() {
        let result = ToolResult::Document {
            format: DocumentFormat::Txt,
            data: vec![0u8; 100],
            name: None,
        };
        let output = format_result_plain(&result);

        assert!(output.contains("unnamed"));
    }

    // ===== ToolResult factory tests =====

    #[test]
    fn test_tool_result_image_factory() {
        let result = ToolResult::image(ImageFormat::Jpeg, vec![1, 2, 3]);

        if let ToolResult::Image { format, data } = result {
            assert_eq!(format, ImageFormat::Jpeg);
            assert_eq!(data, vec![1, 2, 3]);
        } else {
            panic!("Expected Image variant");
        }
    }

    #[test]
    fn test_tool_result_document_factory() {
        let result = ToolResult::document(DocumentFormat::Csv, vec![4, 5, 6]);

        if let ToolResult::Document { format, data, name } = result {
            assert_eq!(format, DocumentFormat::Csv);
            assert_eq!(data, vec![4, 5, 6]);
            assert!(name.is_none());
        } else {
            panic!("Expected Document variant");
        }
    }

    #[test]
    fn test_tool_result_document_with_name_factory() {
        let result = ToolResult::document_with_name(DocumentFormat::Html, vec![7, 8], "page.html");

        if let ToolResult::Document { format, data, name } = result {
            assert_eq!(format, DocumentFormat::Html);
            assert_eq!(data, vec![7, 8]);
            assert_eq!(name, Some("page.html".to_string()));
        } else {
            panic!("Expected Document variant");
        }
    }

    // ===== ToolResult::as_text for binary types =====

    #[test]
    fn test_tool_result_as_text_image() {
        let result = ToolResult::Image {
            format: ImageFormat::Gif,
            data: vec![0u8; 2000],
        };
        let text = result.as_text();

        assert!(text.contains("Image"));
        assert!(text.contains("Gif"));
        assert!(text.contains("2000 bytes"));
    }

    #[test]
    fn test_tool_result_as_text_document() {
        let result = ToolResult::Document {
            format: DocumentFormat::Xlsx,
            data: vec![0u8; 3000],
            name: Some("data.xlsx".to_string()),
        };
        let text = result.as_text();

        assert!(text.contains("Document"));
        assert!(text.contains("Xlsx"));
        assert!(text.contains("data.xlsx"));
        assert!(text.contains("3000 bytes"));
    }

    #[test]
    fn test_tool_result_as_str_binary_types() {
        let image = ToolResult::Image {
            format: ImageFormat::Webp,
            data: vec![],
        };
        assert!(image.as_str().is_none());

        let doc = ToolResult::Document {
            format: DocumentFormat::Doc,
            data: vec![],
            name: None,
        };
        assert!(doc.as_str().is_none());
    }
}
