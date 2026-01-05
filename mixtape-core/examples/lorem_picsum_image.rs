// Example demonstrating tools that return images
//
// This shows how to build a tool that returns image data which
// the model can then analyze or describe. It also demonstrates
// using AgentHook to observe tool call results.
//
// Run with: cargo run --example lorem_picsum_image

use mixtape_core::events::{AgentEvent, AgentHook};
use mixtape_core::{Agent, ImageFormat, Nova2Lite, Tool, ToolError, ToolResult};
use reqwest::Client;
use schemars::JsonSchema;
use serde::Deserialize;
use std::time::Duration;

/// Input for fetching the test image (no parameters needed)
#[derive(Debug, Deserialize, JsonSchema)]
struct GetTestImageInput {}

/// Tool that fetches a fixed test image for verification
struct GetTestImageTool {
    client: Client,
}

impl GetTestImageTool {
    fn new() -> Self {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .expect("Failed to create HTTP client");
        Self { client }
    }
}

impl Tool for GetTestImageTool {
    type Input = GetTestImageInput;

    fn name(&self) -> &str {
        "get_test_image"
    }

    fn description(&self) -> &str {
        "Fetch a test image. Returns the image data which you can then analyze and describe in detail."
    }

    async fn execute(&self, _input: Self::Input) -> Result<ToolResult, ToolError> {
        // Always fetch the same image for reproducibility
        let url = "https://picsum.photos/seed/demo/400/400";

        let response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|e| ToolError::Custom(format!("Failed to fetch image: {}", e)))?;

        if !response.status().is_success() {
            return Err(ToolError::Custom(format!(
                "Failed to fetch image: HTTP {}",
                response.status()
            )));
        }

        let bytes = response
            .bytes()
            .await
            .map_err(|e| ToolError::Custom(format!("Failed to read image data: {}", e)))?;

        Ok(ToolResult::image(ImageFormat::Jpeg, bytes.to_vec()))
    }
}

/// Hook that logs tool execution results
struct ToolResultLogger;

impl AgentHook for ToolResultLogger {
    fn on_event(&self, event: &AgentEvent) {
        match event {
            AgentEvent::ToolRequested { name, .. } => {
                println!("[Hook] Tool '{}' requested...", name);
            }
            AgentEvent::ToolCompleted {
                name,
                output,
                duration,
                ..
            } => {
                // Efficiently access result metadata without copying data
                match output {
                    ToolResult::Image { format, data } => {
                        // data.len() is O(1) - just reads Vec's length field
                        println!(
                            "[Hook] Tool '{}' returned {:?} image: {} bytes (took {:?})",
                            name,
                            format,
                            data.len(),
                            duration
                        );
                    }
                    ToolResult::Document {
                        format,
                        data,
                        name: doc_name,
                    } => {
                        let doc_name = doc_name.as_deref().unwrap_or("unnamed");
                        println!(
                            "[Hook] Tool '{}' returned {:?} document '{}': {} bytes (took {:?})",
                            name,
                            format,
                            doc_name,
                            data.len(),
                            duration
                        );
                    }
                    ToolResult::Text(text) => {
                        println!(
                            "[Hook] Tool '{}' returned text: {} chars (took {:?})",
                            name,
                            text.len(),
                            duration
                        );
                    }
                    ToolResult::Json(value) => {
                        println!(
                            "[Hook] Tool '{}' returned JSON (took {:?}): {}",
                            name,
                            duration,
                            serde_json::to_string(value).unwrap_or_default()
                        );
                    }
                }
            }
            AgentEvent::ToolFailed { name, error, .. } => {
                println!("[Hook] Tool '{}' failed: {}", name, error);
            }
            _ => {}
        }
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("=== Image Tool Example ===\n");
    println!("This example tests whether the model can actually see and describe images");
    println!("returned by tools.\n");
    println!("The test image is available at: https://picsum.photos/seed/demo/400/400");
    println!("It shows a black-and-white photo of water droplets on a rough surface.\n");
    println!("Evaluate how well the model describes what it sees!\n");
    println!("---\n");

    // Create agent with image tool
    let agent = Agent::builder()
        .bedrock(Nova2Lite)
        .add_trusted_tool(GetTestImageTool::new())
        .build()
        .await?;

    // Add hook for observing results (hooks are added post-construction)
    agent.add_hook(ToolResultLogger);

    // Ask the model to fetch and describe the image
    let response = agent
        .run(
            "Use the get_test_image tool to fetch the image, then describe in detail what you see.",
        )
        .await?;

    println!("Model's description:\n");
    println!("{}", response.text);

    Ok(())
}
