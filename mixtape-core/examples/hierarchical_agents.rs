// Example demonstrating hierarchical multi-agent architectures
//
// This shows how specialist agents can be wrapped as tools for an orchestrator agent.
// The orchestrator delegates tasks to specialists based on the domain.
//
// Uses default SlidingWindowConversationManager - best for most agents since it
// automatically manages context window limits by keeping recent messages that fit.
//
// Run with: cargo run --example hierarchical_agents

use mixtape_core::{
    Agent, AgentEvent, BedrockProvider, ClaudeHaiku4_5, InferenceProfile, NovaPro, Tool, ToolError,
    ToolResult,
};

/// Input type for delegating to a specialist agent
///
/// Doc comments on fields become "description" in the JSON schema,
/// helping the orchestrator model understand how to delegate.
#[derive(Debug, serde::Serialize, serde::Deserialize, schemars::JsonSchema)]
struct QueryInput {
    /// The question or task to delegate to the specialist
    query: String,
}

/// A tool that wraps a specialist agent
struct SpecialistTool {
    name: String,
    description: String,
    agent: Agent,
}

impl Tool for SpecialistTool {
    type Input = QueryInput;

    fn name(&self) -> &str {
        &self.name
    }

    fn description(&self) -> &str {
        &self.description
    }

    async fn execute(&self, input: Self::Input) -> Result<ToolResult, ToolError> {
        self.agent
            .run(&input.query)
            .await
            .map(ToolResult::text)
            .map_err(|e| ToolError::Custom(e.to_string()))
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    println!("🎵 Mixtape Hierarchical Agents Example\n");
    println!("Demonstrating orchestrator + specialist pattern\n");
    println!("{}", "=".repeat(60));

    // Create specialist agents
    println!("\n📚 Creating specialist agents...");

    // Using NovaPro for specialists - cheaper and avoids rate limiting
    // US inference profile provides cross-region failover for improved reliability
    let rust_provider = BedrockProvider::new(NovaPro)
        .await?
        .with_inference_profile(InferenceProfile::US)
        .with_retry_callback(|info| {
            eprintln!(
                "  ⚠ Retry {}/{} in {:?} ({})",
                info.attempt, info.max_attempts, info.delay, info.error
            );
        });
    let rust_expert = Agent::builder()
        .provider(rust_provider)
        .with_system_prompt(
            "You are an expert Rust programmer. \
             Provide concise, accurate answers about Rust programming, \
             focusing on best practices, safety, and performance.",
        )
        .build()
        .await?;

    let python_provider = BedrockProvider::new(NovaPro)
        .await?
        .with_inference_profile(InferenceProfile::US)
        .with_retry_callback(|info| {
            eprintln!(
                "  ⚠ Retry {}/{} in {:?} ({})",
                info.attempt, info.max_attempts, info.delay, info.error
            );
        });
    let python_expert = Agent::builder()
        .provider(python_provider)
        .with_system_prompt(
            "You are an expert Python programmer. \
             Provide concise, accurate answers about Python programming, \
             focusing on pythonic idioms, libraries, and best practices.",
        )
        .build()
        .await?;

    println!("  ✓ Created Rust specialist (NovaPro)");
    println!("  ✓ Created Python specialist (NovaPro)");

    // Wrap specialists as tools
    println!("\n🔧 Wrapping specialists as tools...");

    let rust_tool = SpecialistTool {
        name: "rust_specialist".to_string(),
        description: "Delegate Rust programming questions to an expert. Use for: syntax, best practices, crates, performance".to_string(),
        agent: rust_expert,
    };

    let python_tool = SpecialistTool {
        name: "python_specialist".to_string(),
        description: "Delegate Python programming questions to an expert. Use for: syntax, libraries, best practices".to_string(),
        agent: python_expert,
    };

    println!("  ✓ Wrapped as tools");

    // Create orchestrator agent
    println!("\n🎯 Creating orchestrator agent...");

    // Using Haiku for the orchestrator - smart enough to delegate, much cheaper
    let orchestrator_provider = BedrockProvider::new(ClaudeHaiku4_5)
        .await?
        .with_inference_profile(InferenceProfile::US)
        .with_retry_callback(|info| {
            eprintln!(
                "  ⚠ Retry {}/{} in {:?} ({})",
                info.attempt, info.max_attempts, info.delay, info.error
            );
        });
    let orchestrator = Agent::builder()
        .provider(orchestrator_provider)
        .with_system_prompt(
            "You are an orchestrator that coordinates between language specialists. \
             When asked about programming, delegate to the appropriate specialist. \
             For Rust questions, use rust_specialist. For Python questions, use python_specialist. \
             After receiving their answer, you can synthesize or add context if helpful.",
        )
        .add_trusted_tool(rust_tool)
        .add_trusted_tool(python_tool)
        .build()
        .await?;

    // Add hook to show delegation in action
    orchestrator.add_hook(|event: &AgentEvent| match event {
        AgentEvent::ToolStarted { name, .. } => {
            println!("  🔀 Delegating to {}...", name);
        }
        AgentEvent::ToolCompleted { name, duration, .. } => {
            println!("  ✓ {} responded ({:.1}s)", name, duration.as_secs_f64());
        }
        _ => {}
    });

    println!("  ✓ Orchestrator configured with 2 specialist tools");

    // Test delegation
    println!("\n\n{}", "=".repeat(60));
    println!("🧪 Testing hierarchical delegation\n");

    let question = "Compare error handling patterns in Rust and Python. \
                   How does Rust's Result type differ from Python's try/except?";

    println!("Question: {}\n", question);
    println!("🤔 Orchestrator thinking...\n");

    let response = orchestrator.run(question).await?;

    println!("{}", "─".repeat(60));
    println!("📝 Orchestrator Response:\n");
    println!("{}", response.text);
    println!("\n{}", "=".repeat(60));

    // Display execution stats
    println!("\n📊 Execution Stats:");
    println!("   Duration: {:.2}s", response.duration.as_secs_f64());
    println!("   Model calls: {}", response.model_calls);
    println!("   Tool calls: {}", response.tool_calls.len());
    for tc in &response.tool_calls {
        println!(
            "     - {} ({:.2}s) {}",
            tc.name,
            tc.duration.as_secs_f64(),
            if tc.success { "✓" } else { "✗" }
        );
    }

    // Show context usage for the orchestrator
    let usage = orchestrator.get_context_usage();
    println!("\n📝 Context (SlidingWindow default):");
    println!("   Messages: {}", usage.total_messages);
    println!("   Tokens: ~{}", usage.context_tokens);
    println!("   Usage: {:.1}%", usage.usage_percentage * 100.0);

    println!("\n✅ Example complete!");

    Ok(())
}
