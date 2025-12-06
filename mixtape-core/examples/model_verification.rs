//! Model Verification Example
//!
//! Verifies basic agentic flow works across all supported Bedrock models.
//! Runs each model through a standardized test with tool use validation.
//!
//! # Usage
//!
//! ```bash
//! # Run all models
//! cargo run --example model_verification
//!
//! # Run specific providers
//! cargo run --example model_verification -- --providers anthropic,amazon
//!
//! # Run specific models
//! cargo run --example model_verification -- --models ClaudeSonnet4_5,NovaPro
//!
//! # Combine filters
//! cargo run --example model_verification -- --providers meta --models Llama3_3_70B
//! ```
//!
//! # Requirements
//!
//! - AWS credentials configured (via environment, ~/.aws/credentials, or IAM role)
//! - Access to Bedrock models in your AWS region

use mixtape_core::{InferenceProfile, *};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::sync::Arc;
use std::time::{Duration, Instant};

// =============================================================================
// Test Tools
// =============================================================================

/// Input for the calculator tool (returns Text result)
#[derive(Debug, Deserialize, JsonSchema)]
struct CalculatorInput {
    /// Mathematical expression to evaluate (e.g., "2 + 2", "10 * 5")
    expression: String,
}

/// A simple calculator that returns text results
struct Calculator;

impl Tool for Calculator {
    type Input = CalculatorInput;

    fn name(&self) -> &str {
        "calculator"
    }

    fn description(&self) -> &str {
        "Evaluates simple mathematical expressions and returns the result as text"
    }

    async fn execute(&self, input: Self::Input) -> std::result::Result<ToolResult, ToolError> {
        // Simple expression evaluation for testing
        let result = match input.expression.as_str() {
            "2 + 2" | "2+2" => "4",
            "10 * 5" | "10*5" => "50",
            "100 / 4" | "100/4" => "25",
            "7 - 3" | "7-3" => "4",
            _ => "42", // Default for other expressions
        };

        Ok(ToolResult::Text(format!(
            "The result of {} is {}",
            input.expression, result
        )))
    }
}

/// Input for the weather tool (returns JSON result)
#[derive(Debug, Deserialize, JsonSchema)]
struct WeatherInput {
    /// City name to get weather for
    city: String,
}

/// Weather data returned as JSON
#[derive(Debug, Serialize)]
struct WeatherData {
    city: String,
    temperature_celsius: i32,
    condition: String,
    humidity_percent: i32,
}

/// A mock weather tool that returns JSON results
struct Weather;

impl Tool for Weather {
    type Input = WeatherInput;

    fn name(&self) -> &str {
        "get_weather"
    }

    fn description(&self) -> &str {
        "Gets current weather data for a city, returning structured JSON with temperature, condition, and humidity"
    }

    async fn execute(&self, input: Self::Input) -> std::result::Result<ToolResult, ToolError> {
        // Mock weather data
        let data = WeatherData {
            city: input.city.clone(),
            temperature_celsius: 22,
            condition: "Partly cloudy".to_string(),
            humidity_percent: 65,
        };

        Ok(ToolResult::Json(serde_json::to_value(data).unwrap()))
    }
}

// =============================================================================
// Model Registry
// =============================================================================

/// Information about a model for testing
struct ModelInfo {
    /// Unique identifier used in CLI args
    key: &'static str,
    /// Display name
    name: &'static str,
    /// Model vendor (anthropic, amazon, meta, etc.) - all use AWS Bedrock as provider
    vendor: &'static str,
    /// Whether the model supports tool use
    supports_tools: bool,
}

/// All available models for testing
const MODELS: &[ModelInfo] = &[
    // Anthropic Claude
    ModelInfo {
        key: "Claude3_7Sonnet",
        name: "Claude 3.7 Sonnet",
        vendor: "anthropic",
        supports_tools: true,
    },
    ModelInfo {
        key: "ClaudeOpus4",
        name: "Claude Opus 4",
        vendor: "anthropic",
        supports_tools: true,
    },
    ModelInfo {
        key: "ClaudeSonnet4",
        name: "Claude Sonnet 4",
        vendor: "anthropic",
        supports_tools: true,
    },
    ModelInfo {
        key: "ClaudeSonnet4_5",
        name: "Claude Sonnet 4.5",
        vendor: "anthropic",
        supports_tools: true,
    },
    ModelInfo {
        key: "ClaudeHaiku4_5",
        name: "Claude Haiku 4.5",
        vendor: "anthropic",
        supports_tools: true,
    },
    ModelInfo {
        key: "ClaudeOpus4_5",
        name: "Claude Opus 4.5",
        vendor: "anthropic",
        supports_tools: true,
    },
    // Amazon Nova
    ModelInfo {
        key: "NovaMicro",
        name: "Nova Micro",
        vendor: "amazon",
        supports_tools: true,
    },
    ModelInfo {
        key: "NovaLite",
        name: "Nova Lite",
        vendor: "amazon",
        supports_tools: true,
    },
    ModelInfo {
        key: "Nova2Lite",
        name: "Nova 2 Lite",
        vendor: "amazon",
        supports_tools: true,
    },
    ModelInfo {
        key: "NovaPro",
        name: "Nova Pro",
        vendor: "amazon",
        supports_tools: true,
    },
    ModelInfo {
        key: "NovaPremier",
        name: "Nova Premier",
        vendor: "amazon",
        supports_tools: true,
    },
    // Mistral
    ModelInfo {
        key: "MistralLarge3",
        name: "Mistral Large 3",
        vendor: "mistral",
        supports_tools: true,
    },
    ModelInfo {
        key: "MagistralSmall",
        name: "Magistral Small",
        vendor: "mistral",
        supports_tools: true,
    },
    // Cohere
    ModelInfo {
        key: "CohereCommandRPlus",
        name: "Command R+",
        vendor: "cohere",
        supports_tools: true,
    },
    // Qwen
    ModelInfo {
        key: "Qwen3_235B",
        name: "Qwen3 235B",
        vendor: "qwen",
        supports_tools: true,
    },
    ModelInfo {
        key: "Qwen3Coder480B",
        name: "Qwen3 Coder 480B",
        vendor: "qwen",
        supports_tools: true,
    },
    // Google
    ModelInfo {
        key: "Gemma3_27B",
        name: "Gemma 3 27B",
        vendor: "google",
        supports_tools: false, // Gemma doesn't support tool use on Bedrock
    },
    // DeepSeek
    ModelInfo {
        key: "DeepSeekR1",
        name: "DeepSeek R1",
        vendor: "deepseek",
        supports_tools: false, // R1 is reasoning-focused, limited tool support
    },
    ModelInfo {
        key: "DeepSeekV3",
        name: "DeepSeek V3.1",
        vendor: "deepseek",
        supports_tools: true,
    },
    // Moonshot
    ModelInfo {
        key: "KimiK2Thinking",
        name: "Kimi K2 Thinking",
        vendor: "moonshot",
        supports_tools: true,
    },
    // Meta Llama 4
    ModelInfo {
        key: "Llama4Scout17B",
        name: "Llama 4 Scout 17B",
        vendor: "meta",
        supports_tools: true,
    },
    ModelInfo {
        key: "Llama4Maverick17B",
        name: "Llama 4 Maverick 17B",
        vendor: "meta",
        supports_tools: true,
    },
    // Meta Llama 3.3
    ModelInfo {
        key: "Llama3_3_70B",
        name: "Llama 3.3 70B",
        vendor: "meta",
        supports_tools: true,
    },
    // Meta Llama 3.2
    ModelInfo {
        key: "Llama3_2_90B",
        name: "Llama 3.2 90B",
        vendor: "meta",
        supports_tools: true,
    },
    ModelInfo {
        key: "Llama3_2_11B",
        name: "Llama 3.2 11B",
        vendor: "meta",
        supports_tools: true,
    },
    ModelInfo {
        key: "Llama3_2_3B",
        name: "Llama 3.2 3B",
        vendor: "meta",
        supports_tools: false, // AWS Bedrock: "This model doesn't support tool use"
    },
    ModelInfo {
        key: "Llama3_2_1B",
        name: "Llama 3.2 1B",
        vendor: "meta",
        supports_tools: false, // AWS Bedrock: "This model doesn't support tool use"
    },
    // Meta Llama 3.1
    ModelInfo {
        key: "Llama3_1_405B",
        name: "Llama 3.1 405B",
        vendor: "meta",
        supports_tools: true,
    },
    ModelInfo {
        key: "Llama3_1_70B",
        name: "Llama 3.1 70B",
        vendor: "meta",
        supports_tools: true,
    },
    ModelInfo {
        key: "Llama3_1_8B",
        name: "Llama 3.1 8B",
        vendor: "meta",
        supports_tools: true,
    },
];

// =============================================================================
// Test Result Tracking
// =============================================================================

#[derive(Debug)]
#[allow(dead_code)] // model_key used for debugging/filtering
struct TestResult {
    model_key: String,
    model_name: String,
    vendor: String,
    status: TestStatus,
    duration: Duration,
    tool_calls: usize,
    input_tokens: usize,
    output_tokens: usize,
    response_preview: String,
}

#[derive(Debug)]
enum TestStatus {
    Passed,
    Failed(String),
    Skipped(String),
}

// =============================================================================
// Model Creation
// =============================================================================

/// Create a provider with US inference profile for cross-region failover reliability
async fn create_provider(key: &str) -> Option<Arc<dyn ModelProvider>> {
    match key {
        // Anthropic
        "Claude3_7Sonnet" => Some(Arc::new(
            BedrockProvider::new(Claude3_7Sonnet)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        "ClaudeOpus4" => Some(Arc::new(
            BedrockProvider::new(ClaudeOpus4)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        "ClaudeSonnet4" => Some(Arc::new(
            BedrockProvider::new(ClaudeSonnet4)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        "ClaudeSonnet4_5" => Some(Arc::new(
            BedrockProvider::new(ClaudeSonnet4_5)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        "ClaudeHaiku4_5" => Some(Arc::new(
            BedrockProvider::new(ClaudeHaiku4_5)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        "ClaudeOpus4_5" => Some(Arc::new(
            BedrockProvider::new(ClaudeOpus4_5)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        // Amazon Nova
        "NovaMicro" => Some(Arc::new(
            BedrockProvider::new(NovaMicro)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        "NovaLite" => Some(Arc::new(
            BedrockProvider::new(NovaLite)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        "Nova2Lite" => Some(Arc::new(
            BedrockProvider::new(Nova2Lite)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        "NovaPro" => Some(Arc::new(
            BedrockProvider::new(NovaPro)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        "NovaPremier" => Some(Arc::new(
            BedrockProvider::new(NovaPremier)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        // Mistral
        "MistralLarge3" => Some(Arc::new(
            BedrockProvider::new(MistralLarge3)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        "MagistralSmall" => Some(Arc::new(
            BedrockProvider::new(MagistralSmall)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        // Cohere
        "CohereCommandRPlus" => Some(Arc::new(
            BedrockProvider::new(CohereCommandRPlus)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        // Qwen
        "Qwen3_235B" => Some(Arc::new(
            BedrockProvider::new(Qwen3_235B)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        "Qwen3Coder480B" => Some(Arc::new(
            BedrockProvider::new(Qwen3Coder480B)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        // Google
        "Gemma3_27B" => Some(Arc::new(
            BedrockProvider::new(Gemma3_27B)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        // DeepSeek
        "DeepSeekR1" => Some(Arc::new(
            BedrockProvider::new(DeepSeekR1)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        "DeepSeekV3" => Some(Arc::new(
            BedrockProvider::new(DeepSeekV3)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        // Moonshot
        "KimiK2Thinking" => Some(Arc::new(
            BedrockProvider::new(KimiK2Thinking)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        // Meta Llama 4
        "Llama4Scout17B" => Some(Arc::new(
            BedrockProvider::new(Llama4Scout17B)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        "Llama4Maverick17B" => Some(Arc::new(
            BedrockProvider::new(Llama4Maverick17B)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        // Meta Llama 3.3
        "Llama3_3_70B" => Some(Arc::new(
            BedrockProvider::new(Llama3_3_70B)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        // Meta Llama 3.2
        "Llama3_2_90B" => Some(Arc::new(
            BedrockProvider::new(Llama3_2_90B)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        "Llama3_2_11B" => Some(Arc::new(
            BedrockProvider::new(Llama3_2_11B)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        "Llama3_2_3B" => Some(Arc::new(
            BedrockProvider::new(Llama3_2_3B)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        "Llama3_2_1B" => Some(Arc::new(
            BedrockProvider::new(Llama3_2_1B)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        // Meta Llama 3.1
        "Llama3_1_405B" => Some(Arc::new(
            BedrockProvider::new(Llama3_1_405B)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        "Llama3_1_70B" => Some(Arc::new(
            BedrockProvider::new(Llama3_1_70B)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        "Llama3_1_8B" => Some(Arc::new(
            BedrockProvider::new(Llama3_1_8B)
                .await
                .ok()?
                .with_inference_profile(InferenceProfile::US),
        )),
        _ => None,
    }
}

// =============================================================================
// CLI Argument Parsing
// =============================================================================

struct CliArgs {
    vendors: Option<HashSet<String>>,
    models: Option<HashSet<String>>,
    help: bool,
}

fn parse_args() -> CliArgs {
    let args: Vec<String> = std::env::args().collect();
    let mut cli = CliArgs {
        vendors: None,
        models: None,
        help: false,
    };

    let mut i = 1;
    while i < args.len() {
        match args[i].as_str() {
            "--help" | "-h" => {
                cli.help = true;
            }
            "--vendors" => {
                i += 1;
                if i < args.len() {
                    cli.vendors = Some(
                        args[i]
                            .split(',')
                            .map(|s| s.trim().to_lowercase())
                            .collect(),
                    );
                }
            }
            "--models" => {
                i += 1;
                if i < args.len() {
                    cli.models = Some(args[i].split(',').map(|s| s.trim().to_string()).collect());
                }
            }
            _ => {}
        }
        i += 1;
    }

    cli
}

fn print_help() {
    println!(
        r#"Model Verification Example

Verifies basic agentic flow works across all supported AWS Bedrock models.
All models are accessed through AWS Bedrock - no direct vendor API keys needed.

USAGE:
    cargo run --example model_verification [OPTIONS]

OPTIONS:
    -h, --help              Print this help message
    --vendors <LIST>        Comma-separated list of model vendors to test
    --models <LIST>         Comma-separated list of model keys to test

VENDORS (model creators, all accessed via AWS Bedrock):
    anthropic, amazon, mistral, cohere, qwen, google, deepseek, moonshot, meta

MODELS:
    Claude:       Claude3_7Sonnet, ClaudeOpus4, ClaudeSonnet4, ClaudeSonnet4_5,
                  ClaudeHaiku4_5, ClaudeOpus4_5
    Nova:         NovaMicro, NovaLite, Nova2Lite, NovaPro, NovaPremier
    Mistral:      MistralLarge3, MagistralSmall
    Cohere:       CohereCommandRPlus
    Qwen:         Qwen3_235B, Qwen3Coder480B
    Google:       Gemma3_27B
    DeepSeek:     DeepSeekR1, DeepSeekV3
    Moonshot:     KimiK2Thinking
    Llama 4:      Llama4Scout17B, Llama4Maverick17B
    Llama 3.3:    Llama3_3_70B
    Llama 3.2:    Llama3_2_90B, Llama3_2_11B, Llama3_2_3B, Llama3_2_1B
    Llama 3.1:    Llama3_1_405B, Llama3_1_70B, Llama3_1_8B

EXAMPLES:
    # Run all models
    cargo run --example model_verification

    # Run only Claude models
    cargo run --example model_verification -- --vendors anthropic

    # Run specific models
    cargo run --example model_verification -- --models ClaudeSonnet4_5,NovaPro

    # Combine filters (models matching vendor AND in model list)
    cargo run --example model_verification -- --vendors meta --models Llama3_3_70B
"#
    );
}

// =============================================================================
// Test Execution
// =============================================================================

/// Hook to track and display tool calls during test
#[derive(Clone)]
struct VerboseLogger {
    tool_count: Arc<std::sync::atomic::AtomicUsize>,
    input_tokens: Arc<std::sync::atomic::AtomicUsize>,
    output_tokens: Arc<std::sync::atomic::AtomicUsize>,
}

impl VerboseLogger {
    fn new() -> Self {
        Self {
            tool_count: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            input_tokens: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
            output_tokens: Arc::new(std::sync::atomic::AtomicUsize::new(0)),
        }
    }

    fn tool_count(&self) -> usize {
        self.tool_count.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn input_tokens(&self) -> usize {
        self.input_tokens.load(std::sync::atomic::Ordering::SeqCst)
    }

    fn output_tokens(&self) -> usize {
        self.output_tokens.load(std::sync::atomic::Ordering::SeqCst)
    }
}

impl AgentHook for VerboseLogger {
    fn on_event(&self, event: &AgentEvent) {
        match event {
            AgentEvent::ToolStarted { name, input, .. } => {
                println!("     [tool] {} called with:", name);
                // Pretty print the input, indented
                let input_str =
                    serde_json::to_string_pretty(input).unwrap_or_else(|_| input.to_string());
                for line in input_str.lines() {
                    println!("             {}", line);
                }
            }
            AgentEvent::ToolCompleted { name, output, .. } => {
                self.tool_count
                    .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                let output_str = output.as_text();
                println!("     [tool] {} returned:", name);
                for line in output_str.lines() {
                    println!("             {}", line);
                }
            }
            AgentEvent::ToolFailed { name, error, .. } => {
                println!("     [tool] {} FAILED: {}", name, error);
            }
            AgentEvent::ModelCallCompleted {
                response_content,
                stop_reason,
                tokens,
                ..
            } => {
                // Track token usage
                if let Some(usage) = tokens {
                    self.input_tokens
                        .fetch_add(usage.input_tokens, std::sync::atomic::Ordering::SeqCst);
                    self.output_tokens
                        .fetch_add(usage.output_tokens, std::sync::atomic::Ordering::SeqCst);
                    println!(
                        "     [model] tokens: {} input, {} output",
                        usage.input_tokens, usage.output_tokens
                    );
                }
                if let Some(reason) = stop_reason {
                    println!("     [model] stop_reason: {:?}", reason);
                }
                if !response_content.is_empty() {
                    println!("     [model] response:");
                    for line in response_content.lines() {
                        println!("             {}", line);
                    }
                }
            }
            _ => {}
        }
    }
}

async fn run_test(info: &ModelInfo) -> TestResult {
    let start = Instant::now();

    // Skip models that don't support tools
    if !info.supports_tools {
        return TestResult {
            model_key: info.key.to_string(),
            model_name: info.name.to_string(),
            vendor: info.vendor.to_string(),
            status: TestStatus::Skipped("No tool support".to_string()),
            duration: Duration::ZERO,
            tool_calls: 0,
            input_tokens: 0,
            output_tokens: 0,
            response_preview: "-".to_string(),
        };
    }

    println!("\n-> Testing {} ({})...", info.name, info.vendor);

    // Create provider
    let provider = match create_provider(info.key).await {
        Some(p) => p,
        None => {
            return TestResult {
                model_key: info.key.to_string(),
                model_name: info.name.to_string(),
                vendor: info.vendor.to_string(),
                status: TestStatus::Failed("Failed to create provider".to_string()),
                duration: start.elapsed(),
                tool_calls: 0,
                input_tokens: 0,
                output_tokens: 0,
                response_preview: "-".to_string(),
            };
        }
    };

    // Create agent with tools
    let agent = Agent::builder()
        .provider(provider)
        .add_tool(Calculator)
        .add_tool(Weather)
        .build()
        .await
        .unwrap();

    // Add hook for verbose logging
    let logger = VerboseLogger::new();
    agent.add_hook(logger.clone());

    // Test prompt that requires both tools
    let prompt = r#"I need you to help me with two things:
1. Calculate what 2 + 2 equals using the calculator tool
2. Get the current weather in Tokyo using the weather tool

Please use both tools and then summarize the results."#;

    // Show the prompt
    println!("     [prompt]");
    for line in prompt.lines() {
        println!("             {}", line);
    }
    println!();

    // Run agent
    let result = agent.run(prompt).await;
    let duration = start.elapsed();
    let tool_calls = logger.tool_count();
    let input_tokens = logger.input_tokens();
    let output_tokens = logger.output_tokens();

    match result {
        Ok(response) => {
            // Truncate response for preview
            let preview: String = response.text.chars().take(80).collect();
            let preview = if response.text.len() > 80 {
                format!("{}...", preview)
            } else {
                preview
            };

            // Verify tool calls were made
            let status = if tool_calls >= 2 {
                println!(
                    "   OK - {} tool calls, {} tokens ({} in / {} out), {:.2}s",
                    tool_calls,
                    input_tokens + output_tokens,
                    input_tokens,
                    output_tokens,
                    duration.as_secs_f64()
                );
                TestStatus::Passed
            } else {
                println!("   WARN - Only {} tool calls (expected 2+)", tool_calls);
                TestStatus::Failed(format!("Only {} tool calls", tool_calls))
            };

            TestResult {
                model_key: info.key.to_string(),
                model_name: info.name.to_string(),
                vendor: info.vendor.to_string(),
                status,
                duration,
                tool_calls,
                input_tokens,
                output_tokens,
                response_preview: preview,
            }
        }
        Err(e) => {
            let error_string = e.to_string();
            println!("   FAIL - {}", truncate_error(&error_string));
            TestResult {
                model_key: info.key.to_string(),
                model_name: info.name.to_string(),
                vendor: info.vendor.to_string(),
                status: TestStatus::Failed(error_string),
                duration,
                tool_calls,
                input_tokens,
                output_tokens,
                response_preview: "-".to_string(),
            }
        }
    }
}

fn truncate_error(e: &str) -> String {
    // Show more of the error for debugging
    if e.len() > 200 {
        format!("{}...", &e[..200])
    } else {
        e.to_string()
    }
}

// =============================================================================
// Results Table
// =============================================================================

fn print_results_table(results: &[TestResult]) {
    println!("\n");
    println!("{}", "=".repeat(140));
    println!("RESULTS SUMMARY");
    println!("{}", "=".repeat(140));
    println!();

    // Header
    println!(
        "{:<25} {:<12} {:<10} {:>10} {:>8} {:>12} {:>12} {:<}",
        "Model", "Vendor", "Status", "Duration", "Tools", "Input Tok", "Output Tok", "Response"
    );
    println!("{}", "-".repeat(140));

    // Results
    for r in results {
        let status_str = match &r.status {
            TestStatus::Passed => "PASS".to_string(),
            TestStatus::Failed(_) => "FAIL".to_string(),
            TestStatus::Skipped(reason) => format!("SKIP ({})", reason),
        };

        let duration_str = if r.duration.as_millis() > 0 {
            format!("{:.2}s", r.duration.as_secs_f64())
        } else {
            "-".to_string()
        };

        let input_tok_str = if r.input_tokens > 0 {
            format!("{}", r.input_tokens)
        } else {
            "-".to_string()
        };

        let output_tok_str = if r.output_tokens > 0 {
            format!("{}", r.output_tokens)
        } else {
            "-".to_string()
        };

        println!(
            "{:<25} {:<12} {:<10} {:>10} {:>8} {:>12} {:>12} {:<}",
            r.model_name,
            r.vendor,
            status_str,
            duration_str,
            r.tool_calls,
            input_tok_str,
            output_tok_str,
            truncate_str(&r.response_preview, 35)
        );
    }

    println!("{}", "-".repeat(140));

    // Summary counts
    let passed = results
        .iter()
        .filter(|r| matches!(r.status, TestStatus::Passed))
        .count();
    let failed = results
        .iter()
        .filter(|r| matches!(r.status, TestStatus::Failed(_)))
        .count();
    let skipped = results
        .iter()
        .filter(|r| matches!(r.status, TestStatus::Skipped(_)))
        .count();

    let total_duration: Duration = results.iter().map(|r| r.duration).sum();
    let total_tools: usize = results.iter().map(|r| r.tool_calls).sum();
    let total_input_tokens: usize = results.iter().map(|r| r.input_tokens).sum();
    let total_output_tokens: usize = results.iter().map(|r| r.output_tokens).sum();

    println!();
    println!(
        "Total: {} passed, {} failed, {} skipped",
        passed, failed, skipped
    );
    println!(
        "Time: {:.2}s | Tool calls: {} | Tokens: {} ({} in / {} out)",
        total_duration.as_secs_f64(),
        total_tools,
        total_input_tokens + total_output_tokens,
        total_input_tokens,
        total_output_tokens
    );
    println!();

    // Show failures if any
    let failures: Vec<_> = results
        .iter()
        .filter_map(|r| {
            if let TestStatus::Failed(e) = &r.status {
                Some((r.model_name.as_str(), e.as_str()))
            } else {
                None
            }
        })
        .collect();

    if !failures.is_empty() {
        println!("FAILURES:");
        for (model, error) in failures {
            println!("  {}: {}", model, truncate_str(error, 80));
        }
        println!();
    }
}

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() > max {
        format!("{}...", &s[..max])
    } else {
        s.to_string()
    }
}

// =============================================================================
// Main
// =============================================================================

#[tokio::main]
async fn main() {
    let args = parse_args();

    if args.help {
        print_help();
        return;
    }

    println!("{}", "=".repeat(60));
    println!("Mixtape Model Verification");
    println!("{}", "=".repeat(60));

    // Filter models based on CLI args
    let models_to_test: Vec<&ModelInfo> = MODELS
        .iter()
        .filter(|m| {
            // Check vendor filter
            let vendor_match = args
                .vendors
                .as_ref()
                .map(|v| v.contains(&m.vendor.to_lowercase()))
                .unwrap_or(true);

            // Check model filter
            let model_match = args
                .models
                .as_ref()
                .map(|models| models.contains(&m.key.to_string()))
                .unwrap_or(true);

            vendor_match && model_match
        })
        .collect();

    if models_to_test.is_empty() {
        println!("\nNo models match the specified filters.");
        println!("Use --help to see available options.");
        return;
    }

    println!("\nTesting {} models...", models_to_test.len());

    // List models to be tested
    println!("\nModels:");
    for m in &models_to_test {
        let tools_indicator = if m.supports_tools { "+" } else { "-" };
        println!("  [{}] {} ({})", tools_indicator, m.name, m.vendor);
    }

    // Run tests sequentially
    let mut results = Vec::new();
    for info in models_to_test {
        let result = run_test(info).await;
        results.push(result);
    }

    // Print results table
    print_results_table(&results);
}
