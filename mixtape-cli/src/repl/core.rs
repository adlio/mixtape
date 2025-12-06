//! Core REPL utilities

use crate::error::CliError;
use mixtape_core::Agent;
use std::io::Write;

/// ANSI escape code to reset terminal styling
pub const RESET_STYLE: &str = "\x1b[0m";

/// The input prompt string
pub fn input_prompt() -> &'static str {
    "  ❯ "
}

/// Format the welcome banner header
pub fn format_welcome_header() -> String {
    format!("🎵 mixtape v{}", env!("CARGO_PKG_VERSION"))
}

/// Format session info for display
pub fn format_session_info(id: &str, message_count: usize) -> String {
    let short_id = &id[..8.min(id.len())];
    format!("Session: {} ({} messages)", short_id, message_count)
}

/// Print padding before input (currently a no-op, but available for customization)
pub fn print_input_padding() {
    let _ = std::io::stdout().flush();
}

/// Reset terminal styling after input
pub fn reset_input_style() {
    let mut stdout = std::io::stdout();
    let _ = write!(stdout, "{}", RESET_STYLE);
    let _ = stdout.flush();
}

/// Format the tip line shown at startup
pub fn format_tip() -> &'static str {
    "Type /help for commands, /tools to list tools, Ctrl+J for multiline"
}

/// Print welcome message and session info
pub async fn print_welcome(agent: &Agent) -> Result<(), CliError> {
    println!("\n{}", format_welcome_header());
    println!("Model: {}", agent.model_name());

    if let Some(info) = agent.get_session_info().await? {
        println!("{}", format_session_info(&info.id, info.message_count));
    }

    println!("{}", format_tip());
    println!();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn input_prompt_returns_expected_string() {
        assert_eq!(input_prompt(), "  ❯ ");
    }

    #[test]
    fn input_prompt_has_leading_spaces() {
        assert!(input_prompt().starts_with("  "));
    }

    #[test]
    fn reset_style_is_ansi_reset() {
        assert_eq!(RESET_STYLE, "\x1b[0m");
    }

    #[test]
    fn format_welcome_header_contains_version() {
        let header = format_welcome_header();
        assert!(header.contains("mixtape"));
        assert!(header.contains("v"));
        // Should contain the actual version from Cargo.toml
        assert!(header.contains(env!("CARGO_PKG_VERSION")));
    }

    #[test]
    fn format_welcome_header_has_emoji() {
        let header = format_welcome_header();
        assert!(header.starts_with("🎵"));
    }

    #[test]
    fn format_session_info_truncates_long_id() {
        let info = format_session_info("abcdefghijklmnop", 5);
        assert_eq!(info, "Session: abcdefgh (5 messages)");
    }

    #[test]
    fn format_session_info_handles_short_id() {
        let info = format_session_info("abc", 10);
        assert_eq!(info, "Session: abc (10 messages)");
    }

    #[test]
    fn format_session_info_shows_message_count() {
        let info = format_session_info("test1234", 42);
        assert!(info.contains("42 messages"));
    }

    #[test]
    fn format_session_info_zero_messages() {
        let info = format_session_info("newid123", 0);
        assert!(info.contains("0 messages"));
    }

    #[test]
    fn format_tip_mentions_key_commands() {
        let tip = format_tip();
        assert!(tip.contains("/help"));
        assert!(tip.contains("/tools"));
        assert!(tip.contains("Ctrl+J"));
    }
}
