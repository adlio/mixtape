// Example demonstrating Tool Search with deferred tool loading
//
// This shows how to use the tool search feature to:
// - Mark tools as "deferred" so they're not loaded into context initially
// - Let Claude discover tools dynamically via search
// - Reduce context usage when you have many tools (30+)
//
// The tool search feature is useful when you have a large catalog of tools
// and want to save context tokens by only loading tool definitions when needed.
//
// Run with: cargo run --example tool_search

use mixtape_core::{
    Agent, BedrockProvider, ClaudeHaiku4_5, InferenceProfile, Tool, ToolError, ToolResult,
    ToolSearchType,
};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

// ============================================================================
// Example Tools - A catalog of tools that can be discovered via search
// ============================================================================

/// Input for the calculator tool
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct CalculatorInput {
    /// Mathematical expression to evaluate (e.g., "2 + 2")
    expression: String,
}

/// A simple calculator tool
struct Calculator;

impl Tool for Calculator {
    type Input = CalculatorInput;

    fn name(&self) -> &str {
        "calculator"
    }

    fn description(&self) -> &str {
        "Evaluate a mathematical expression. Supports basic arithmetic operations."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        // In a real implementation, you'd parse and evaluate the expression
        // For demo purposes, we just echo the expression
        Ok(ToolResult::text(format!(
            "Calculated result for: {}",
            input.expression
        )))
    }
}

/// Input for the email tool
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct SendEmailInput {
    /// Email recipient address
    to: String,
    /// Email subject line
    subject: String,
    /// Email body content
    body: String,
}

/// A tool for sending emails (demo - doesn't actually send)
struct EmailTool;

impl Tool for EmailTool {
    type Input = SendEmailInput;

    fn name(&self) -> &str {
        "send_email"
    }

    fn description(&self) -> &str {
        "Send an email to a recipient. Composes and delivers email messages."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        // In a real implementation, you'd send the email
        Ok(ToolResult::text(format!(
            "Email sent to {} with subject: {}",
            input.to, input.subject
        )))
    }
}

/// Input for the calendar tool
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct CreateEventInput {
    /// Event title
    title: String,
    /// Event date and time (ISO format)
    datetime: String,
    /// Event duration in minutes
    duration_minutes: u32,
}

/// A tool for creating calendar events (demo)
struct CalendarTool;

impl Tool for CalendarTool {
    type Input = CreateEventInput;

    fn name(&self) -> &str {
        "create_calendar_event"
    }

    fn description(&self) -> &str {
        "Create a calendar event with title, date/time, and duration."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        Ok(ToolResult::text(format!(
            "Created event '{}' at {} for {} minutes",
            input.title, input.datetime, input.duration_minutes
        )))
    }
}

/// Input for the file search tool
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
struct FileSearchInput {
    /// Search pattern or query
    query: String,
    /// Directory to search in
    directory: Option<String>,
}

/// A tool for searching files (demo)
struct FileSearchTool;

impl Tool for FileSearchTool {
    type Input = FileSearchInput;

    fn name(&self) -> &str {
        "search_files"
    }

    fn description(&self) -> &str {
        "Search for files matching a pattern in a directory."
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        let dir = input.directory.unwrap_or_else(|| ".".to_string());
        Ok(ToolResult::text(format!(
            "Found files matching '{}' in {}",
            input.query, dir
        )))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Tool Search Example ===\n");
    println!("This example demonstrates deferred tool loading.\n");
    println!("- Calculator is always loaded (frequently used)");
    println!("- Email, Calendar, and File Search are deferred (discovered via search)\n");

    // Create provider
    let provider = BedrockProvider::new(ClaudeHaiku4_5)
        .await?
        .with_inference_profile(InferenceProfile::US)
        .with_tool_search(); // Enable tool search beta

    // Build agent with mix of regular and deferred tools
    let agent = Agent::builder()
        .provider(provider)
        .with_system_prompt(
            "You are a helpful assistant with access to various tools. \
             Use the appropriate tool to help the user.",
        )
        // Calculator is always loaded (frequently used)
        .add_trusted_tool(Calculator)
        // These tools are deferred - discovered via tool search when needed
        .add_deferred_tool(EmailTool)
        .add_deferred_tool(CalendarTool)
        .add_deferred_tool(FileSearchTool)
        // Use regex search (more precise pattern matching)
        .with_tool_search_type(ToolSearchType::Regex)
        .build()
        .await?;

    // List all tools (including deferred ones)
    println!("Registered tools:");
    for tool in agent.list_tools() {
        println!("  - {}: {}", tool.name, tool.description);
    }
    println!();

    // Test with a calculation (uses always-loaded tool)
    let question = "What is 15 * 7?";
    println!("Question: {}\n", question);

    let response = agent.run(question).await?;
    println!("Response: {}\n", response.text);

    // Show stats
    println!("Stats:");
    println!("  Duration: {:.2}s", response.duration.as_secs_f64());
    println!("  Model calls: {}", response.model_calls);
    println!("  Tool calls: {}", response.tool_calls.len());

    for tc in &response.tool_calls {
        println!(
            "    - {} ({:.2}s) {}",
            tc.name,
            tc.duration.as_secs_f64(),
            if tc.success { "✓" } else { "✗" }
        );
    }

    println!("\n---\n");

    // Test with an email request (would trigger tool search for deferred tool)
    let question = "Send an email to bob@example.com saying hello";
    println!("Question: {}\n", question);
    println!("(Note: In production, Claude would use tool search to discover 'send_email')\n");

    // Note: Tool search is a server-side feature. When Claude needs a tool it
    // doesn't have loaded, it will invoke the tool_search_tool to find matching
    // tools. The API then expands the tool references into full definitions.

    let response = agent.run(question).await?;
    println!("Response: {}\n", response.text);

    println!("Stats:");
    println!("  Duration: {:.2}s", response.duration.as_secs_f64());
    println!("  Tool calls: {}", response.tool_calls.len());

    for tc in &response.tool_calls {
        println!(
            "    - {} ({:.2}s) {}",
            tc.name,
            tc.duration.as_secs_f64(),
            if tc.success { "✓" } else { "✗" }
        );
    }

    Ok(())
}
