//! AWS Bedrock AgentCore runtime support.
//!
//! This module implements the [AgentCore HTTP protocol contract](https://docs.aws.amazon.com/bedrock-agentcore/latest/devguide/runtime-http-protocol-contract.html),
//! enabling mixtape agents to run as serverless workloads on AgentCore.
//!
//! # Protocol
//!
//! AgentCore expects containers to expose two HTTP endpoints on port 8080:
//!
//! | Endpoint | Method | Purpose |
//! |----------|--------|---------|
//! | `/ping` | GET | Health check (returns `{"status": "Healthy"}`) |
//! | `/invocations` | POST | Agent execution (returns SSE stream) |
//!
//! # Streaming Events
//!
//! The `/invocations` endpoint returns a Server-Sent Events stream of
//! [`AgentCoreEvent`](events::AgentCoreEvent)s that provide real-time
//! visibility into agent execution:
//!
//! ```text
//! data: {"type":"run_started"}
//! data: {"type":"content_delta","text":"Hello"}
//! data: {"type":"content_delta","text":" world!"}
//! data: {"type":"tool_call_start","tool_call_id":"tc-1","name":"search"}
//! data: {"type":"tool_call_input","tool_call_id":"tc-1","input":{"query":"rust"}}
//! data: {"type":"tool_call_end","tool_call_id":"tc-1"}
//! data: {"type":"tool_call_result","tool_call_id":"tc-1","content":"Found 42 results"}
//! data: {"type":"content_delta","text":"I found "}
//! data: {"type":"content_delta","text":"42 results."}
//! data: {"type":"run_finished","response":"I found 42 results."}
//! ```
//!
//! # Example
//!
//! ```rust,no_run
//! use mixtape_server::MixtapeRouter;
//! use mixtape_core::Agent;
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! # let agent: Agent = todo!();
//! let app = MixtapeRouter::new(agent)
//!     .with_agentcore()
//!     .build()?;
//!
//! // AgentCore expects port 8080
//! let listener = tokio::net::TcpListener::bind("0.0.0.0:8080").await?;
//! axum::serve(listener, app).await?;
//! # Ok(())
//! # }
//! ```

pub mod convert;
pub mod events;
pub mod handler;
