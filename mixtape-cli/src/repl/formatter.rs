//! Tool formatting trait for testability
//!
//! This module provides a trait abstraction over Agent's formatting methods,
//! enabling PresentationHook to be tested with mock implementations.

use mixtape_core::{Agent, Display, ToolResult};
use serde_json::Value;

/// Trait for formatting tool inputs and outputs for display
///
/// This trait abstracts the formatting methods used by [`super::presentation::PresentationHook`],
/// allowing tests to inject mock formatters instead of requiring a real Agent.
pub trait ToolFormatter: Send + Sync {
    /// Format tool input parameters for display
    ///
    /// Returns a formatted string if formatting is available, None otherwise.
    fn format_tool_input(&self, name: &str, input: &Value, display: Display) -> Option<String>;

    /// Format tool output for display
    ///
    /// Returns a formatted string for the tool result.
    fn format_tool_output(
        &self,
        name: &str,
        output: &ToolResult,
        display: Display,
    ) -> Option<String>;
}

/// Implement ToolFormatter for the real Agent
impl ToolFormatter for Agent {
    fn format_tool_input(&self, name: &str, input: &Value, display: Display) -> Option<String> {
        Agent::format_tool_input(self, name, input, display)
    }

    fn format_tool_output(
        &self,
        name: &str,
        output: &ToolResult,
        display: Display,
    ) -> Option<String> {
        Agent::format_tool_output(self, name, output, display)
    }
}
