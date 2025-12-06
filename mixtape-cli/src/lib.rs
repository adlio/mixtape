//! CLI utilities and session management for mixtape
//!
//! This crate provides:
//! - SQLite-based session storage for conversation memory
//! - Interactive REPL/CLI for agent usage
//! - Command history and special commands

mod error;
pub mod repl;
pub mod session;

pub use error::CliError;
pub use repl::{
    indent_lines, print_confirmation, print_tool_header, prompt_for_approval, read_input, run_cli,
    ApprovalPrompter, DefaultPrompter, PermissionRequest, PresentationHook, SimplePrompter,
    Verbosity,
};
pub use session::SqliteStore;
