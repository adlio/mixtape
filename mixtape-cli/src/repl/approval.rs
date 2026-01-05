//! Tool approval handler for CLI
//!
//! Provides simple approval prompts for tool execution permissions.
//! The v1 model offers:
//! - Approve once (don't remember)
//! - Trust this exact call (session only)
//! - Trust the entire tool (session only)
//! - Deny

use mixtape_core::permission::{AuthorizationResponse, Grant, Scope};
use std::io::{stdout, BufRead, Write};

// =============================================================================
// Core Types
// =============================================================================

/// All information needed to prompt for permission approval
#[derive(Debug, Clone)]
pub struct PermissionRequest {
    /// Tool name
    pub tool_name: String,
    /// Tool use ID (for responding to the agent)
    pub tool_use_id: String,
    /// Hash of parameters (for exact-match grants)
    pub params_hash: String,
    /// Formatted display of the full tool call (if available)
    pub formatted_display: Option<String>,
}

/// Trait for approval prompt implementations
///
/// Implement this to create custom approval UX.
pub trait ApprovalPrompter: Send + Sync {
    /// Prompt the user and return their choice
    fn prompt(&self, request: &PermissionRequest) -> AuthorizationResponse;

    /// Human-readable name for this prompter
    fn name(&self) -> &'static str;
}

// =============================================================================
// Default Prompter Implementation
// =============================================================================

/// Simple approval prompter with clear options
///
/// Displays:
/// - y: approve once
/// - e: trust this exact call
/// - t: trust entire tool
/// - n: deny
pub struct SimplePrompter;

impl ApprovalPrompter for SimplePrompter {
    fn name(&self) -> &'static str {
        "SimplePrompter"
    }

    fn prompt(&self, _request: &PermissionRequest) -> AuthorizationResponse {
        // Tool name and params already displayed by ToolRequested event
        println!("│");
        println!(
            "│  \x1b[33mApprove?\x1b[0m  \x1b[2m(y)es  (n)o  (t)rust tool  (e)xact match\x1b[0m"
        );

        loop {
            print!("│  > ");
            let _ = stdout().flush();

            let input = read_input();
            let input = input.trim().to_lowercase();

            match input.as_str() {
                "y" | "yes" => {
                    return AuthorizationResponse::Once;
                }
                "e" | "exact" => {
                    let grant = Grant::exact(&_request.tool_name, &_request.params_hash)
                        .with_scope(Scope::Session);
                    return AuthorizationResponse::Trust { grant };
                }
                "t" | "tool" | "trust" => {
                    let grant = Grant::tool(&_request.tool_name).with_scope(Scope::Session);
                    return AuthorizationResponse::Trust { grant };
                }
                "n" | "no" | "deny" => {
                    return AuthorizationResponse::Deny { reason: None };
                }
                "" => continue,
                _ => {
                    println!("│  \x1b[31mUse y/n/t/e\x1b[0m");
                }
            }
        }
    }
}

/// Default prompter type
pub type DefaultPrompter = SimplePrompter;

// =============================================================================
// Helper Functions
// =============================================================================

/// Read a line of input
pub fn read_input() -> String {
    let stdin = std::io::stdin();
    let mut line = String::new();
    let _ = stdin.lock().read_line(&mut line);
    line
}

/// Print a confirmation message
pub fn print_confirmation(message: &str) {
    println!("  \x1b[32m✓\x1b[0m {}", message);
}

/// Convenience function using the default prompter
pub fn prompt_for_approval(request: &PermissionRequest) -> AuthorizationResponse {
    SimplePrompter.prompt(request)
}
