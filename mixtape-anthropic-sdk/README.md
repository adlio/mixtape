# Mixtape Anthropic SDK

A minimal Anthropic API client. Supports messages, streaming, tools, batching, and token counting.

Most mixtape users should use the main `mixtape-core` crate with the `anthropic` feature instead. This crate is the low-level client that powers it.

## Quick Start

```rust
use mixtape_anthropic_sdk::{Anthropic, MessageCreateParams};

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let client = Anthropic::from_env()?;  // Uses ANTHROPIC_API_KEY

    let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
        .user("Hello, Claude!")
        .build();

    let response = client.messages().create(params).await?;
    println!("{:?}", response);
    Ok(())
}
```

## Streaming

For long responses, streaming provides better feedback:

```rust
let stream = client.messages().stream(params).await?;
let text = stream.collect_text().await?;

// Or get the full message with stop reason and usage
let stream = client.messages().stream(params).await?;
let message = stream.collect_message().await?;
```

## Tools

```rust
use mixtape_anthropic_sdk::{Tool, ToolInputSchema, ToolChoice, ContentBlock};

let tool = Tool {
    name: "get_weather".to_string(),
    description: Some("Get weather for a location".to_string()),
    input_schema: ToolInputSchema::new(),
    cache_control: None,
    tool_type: None,
};

let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 1024)
    .user("What's the weather in Tokyo?")
    .tools(vec![tool])
    .tool_choice(ToolChoice::auto())
    .build();

let response = client.messages().create(params).await?;

for block in &response.content {
    if let ContentBlock::ToolUse { name, input, .. } = block {
        println!("{}: {}", name, input);
    }
}
```

## Extended Thinking

For complex reasoning:

```rust
let params = MessageCreateParams::builder("claude-sonnet-4-20250514", 16000)
    .user("Solve this problem...")
    .thinking(4096)  // Budget for thinking
    .build();
```

## Rate Limits

Access rate limit headers for debugging:

```rust
let response = client.messages().create_with_metadata(params).await?;

if let Some(rate_limit) = response.rate_limit() {
    println!("Remaining: {:?}", rate_limit.requests_remaining);
}
if let Some(request_id) = response.request_id() {
    println!("Request ID: {}", request_id);
}
```

## Retry Configuration

```rust
use mixtape_anthropic_sdk::RetryConfig;
use std::time::Duration;

let client = Anthropic::builder()
    .api_key("your-api-key")
    .max_retries(5)
    .build()?;

// Full control
let client = Anthropic::builder()
    .api_key("your-api-key")
    .retry_config(RetryConfig {
        max_retries: 3,
        base_delay: Duration::from_millis(500),
        max_delay: Duration::from_secs(10),
        jitter: 0.25,
    })
    .build()?;
```

## Batching

For high-volume workloads:

```rust
use mixtape_anthropic_sdk::{BatchCreateParams, BatchRequest};

let requests = vec![
    BatchRequest::new("req-1", params1),
    BatchRequest::new("req-2", params2),
];

let batch = client.batches().create(BatchCreateParams::new(requests)).await?;
println!("Batch ID: {}", batch.id);
```

## Features

| Feature | Description |
|---------|-------------|
| `schemars` | Enable JsonSchema derives for tool inputs |
