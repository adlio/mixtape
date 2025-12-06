//! AWS service integration tools for mixtape agents.
//!
//! This module provides a universal interface to AWS services, allowing agents to
//! invoke any AWS API operation dynamically. It uses SigV4 signing for authentication
//! and supports all AWS credential sources (environment variables, profiles, IAM roles, etc.).
//!
//! # Example
//!
//! ```no_run
//! use mixtape_core::Tool;
//! use mixtape_tools::aws::UseAwsTool;
//!
//! #[tokio::main]
//! async fn main() -> Result<(), Box<dyn std::error::Error>> {
//!     let tool = UseAwsTool::new().await?;
//!
//!     // The tool is typically used through an agent, but can be called directly:
//!     let input = serde_json::from_value(serde_json::json!({
//!         "service_name": "sts",
//!         "operation_name": "GetCallerIdentity",
//!         "parameters": {},
//!         "region": "us-east-1",
//!         "label": "Get AWS caller identity"
//!     }))?;
//!
//!     let result = tool.execute(input).await?;
//!     println!("{}", result.as_text());
//!     Ok(())
//! }
//! ```

mod use_aws;

pub use use_aws::{UseAwsInput, UseAwsTool, UseAwsToolBuilder};
