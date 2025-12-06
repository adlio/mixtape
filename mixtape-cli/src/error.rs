//! CLI-specific error types

use thiserror::Error;

/// Errors that can occur during CLI operations
#[derive(Debug, Error)]
pub enum CliError {
    /// Agent execution error
    #[error("Agent error: {0}")]
    Agent(#[from] mixtape_core::AgentError),

    /// Session storage error
    #[error("Session error: {0}")]
    Session(#[from] mixtape_core::SessionError),

    /// Readline/input error
    #[error("Input error: {0}")]
    Readline(#[from] rustyline::error::ReadlineError),

    /// IO error (filesystem, stdout, etc.)
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    /// Shell command execution error
    #[error("Shell command failed: {0}")]
    ShellCommand(String),
}
