use crate::error::CliError;
use mixtape_core::Agent;
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verbosity {
    Quiet,
    Normal,
    Verbose,
}

impl Verbosity {
    /// Parse a verbosity level from a string
    ///
    /// Returns Some(Verbosity) for valid inputs, None for invalid.
    pub fn parse(s: &str) -> Option<Self> {
        match s {
            "quiet" => Some(Self::Quiet),
            "normal" => Some(Self::Normal),
            "verbose" => Some(Self::Verbose),
            _ => None,
        }
    }
}

/// Classify an input line as a special command type
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommandType<'a> {
    /// Shell command starting with !
    Shell(&'a str),
    /// Slash command with name and arguments
    Slash {
        command: &'a str,
        args: Vec<&'a str>,
    },
    /// Regular input to send to agent
    Regular,
}

impl<'a> CommandType<'a> {
    /// Parse an input line into a command type
    pub fn parse(input: &'a str) -> Self {
        if let Some(shell_cmd) = input.strip_prefix('!') {
            return Self::Shell(shell_cmd);
        }

        if input.starts_with('/') {
            let parts: Vec<&str> = input.split_whitespace().collect();
            if !parts.is_empty() {
                return Self::Slash {
                    command: parts[0],
                    args: parts[1..].to_vec(),
                };
            }
        }

        Self::Regular
    }
}

pub enum SpecialCommandResult {
    Exit,
    Continue,
}

/// Handle special commands (! and /)
///
/// Returns Some(result) if this was a special command,
/// None if it should be sent to the agent.
pub async fn handle_special_command(
    input: &str,
    agent: &Agent,
    verbosity: &Arc<Mutex<Verbosity>>,
) -> Result<Option<SpecialCommandResult>, CliError> {
    match CommandType::parse(input) {
        CommandType::Shell(shell_cmd) => {
            execute_shell_command(shell_cmd).await?;
            Ok(Some(SpecialCommandResult::Continue))
        }
        CommandType::Slash { command, args } => {
            let args = args.as_slice();
            match command {
                "/exit" | "/quit" => Ok(Some(SpecialCommandResult::Exit)),
                "/help" => {
                    show_help();
                    Ok(Some(SpecialCommandResult::Continue))
                }
                "/tools" => {
                    show_tools(agent);
                    Ok(Some(SpecialCommandResult::Continue))
                }
                "/history" => {
                    show_history(agent, args).await?;
                    Ok(Some(SpecialCommandResult::Continue))
                }
                "/clear" => {
                    clear_session(agent).await?;
                    Ok(Some(SpecialCommandResult::Continue))
                }
                "/verbosity" => {
                    update_verbosity(verbosity, args);
                    Ok(Some(SpecialCommandResult::Continue))
                }
                "/session" => {
                    show_session_info(agent).await?;
                    Ok(Some(SpecialCommandResult::Continue))
                }
                _ => {
                    eprintln!(
                        "Unknown command: {}. Type /help for available commands.",
                        command
                    );
                    Ok(Some(SpecialCommandResult::Continue))
                }
            }
        }
        CommandType::Regular => Ok(None),
    }
}

async fn execute_shell_command(cmd: &str) -> Result<(), CliError> {
    println!("\n💻 Executing: {}\n", cmd);

    let mut child = Command::new("sh")
        .arg("-c")
        .arg(cmd)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::piped())
        .spawn()?;

    // Stream stdout
    if let Some(stdout) = child.stdout.take() {
        let reader = BufReader::new(stdout);
        let mut lines = reader.lines();

        while let Some(line) = lines.next_line().await? {
            println!("{}", line);
        }
    }

    // Stream stderr
    if let Some(stderr) = child.stderr.take() {
        let reader = BufReader::new(stderr);
        let mut lines = reader.lines();

        while let Some(line) = lines.next_line().await? {
            eprintln!("{}", line);
        }
    }

    let status = child.wait().await?;

    if !status.success() {
        eprintln!("\n❌ Command exited with status: {}", status);
    }

    println!();
    Ok(())
}

async fn clear_session(agent: &Agent) -> Result<(), CliError> {
    agent.clear_session().await?;
    println!("Session cleared.");
    Ok(())
}

/// Help text sections for the CLI
pub mod help {
    /// Header for the help display
    pub const HEADER: &str = "\n📖 Available Commands:\n";

    /// Shell commands section
    pub const SHELL_COMMANDS: &str = "\
Shell Commands:
  !<command>        Execute shell command and stream output
  Example: !ls -la
";

    /// Navigation commands section
    pub const NAVIGATION: &str = "\
Navigation:
  /help             Show this help message
  /tools            List all available tools
  /history [n]      Show last n messages (default: 10)
  /clear            Clear current session history
  /verbosity [level]  Set output verbosity (quiet|normal|verbose)
";

    /// Session management section
    pub const SESSION: &str = "\
Session Management:
  /session          Show current session info
";

    /// Exit commands section
    pub const EXIT: &str = "\
Exit:
  /exit, /quit      Exit and save session
  Ctrl+C            Interrupt current operation
  Ctrl+D            Exit
";

    /// Keyboard shortcuts section
    pub const KEYBOARD: &str = "\
Keyboard Shortcuts:
  Up/Down           Navigate command history
  Ctrl+R            Reverse search history
  Ctrl+C            Interrupt (doesn't exit)
  Ctrl+D            Exit
";

    /// Get the complete help text
    pub fn full_text() -> String {
        format!(
            "{}{}\n{}\n{}\n{}\n{}",
            HEADER, SHELL_COMMANDS, NAVIGATION, SESSION, EXIT, KEYBOARD
        )
    }
}

fn show_help() {
    print!("{}", help::full_text());
}

/// Tool info for display purposes
pub struct ToolDisplay {
    pub name: String,
    pub description: String,
}

/// Format a list of tools for display
pub fn format_tool_list(tools: &[ToolDisplay]) -> String {
    let mut output = String::from("\n🔧 Available Tools:\n\n");

    if tools.is_empty() {
        output.push_str("  No tools configured\n");
    } else {
        for tool in tools {
            output.push_str(&format!("  {} - {}\n", tool.name, tool.description));
        }
    }

    output
}

fn show_tools(agent: &Agent) {
    let tools: Vec<ToolDisplay> = agent
        .list_tools()
        .into_iter()
        .map(|t| ToolDisplay {
            name: t.name.clone(),
            description: t.description.clone(),
        })
        .collect();

    print!("{}", format_tool_list(&tools));
}

fn update_verbosity(verbosity: &Arc<Mutex<Verbosity>>, args: &[&str]) {
    if args.is_empty() {
        let current = *verbosity.lock().unwrap();
        println!("Verbosity: {:?}", current);
        return;
    }

    match Verbosity::parse(args[0]) {
        Some(level) => {
            *verbosity.lock().unwrap() = level;
            println!("Verbosity set to {:?}", level);
        }
        None => {
            println!(
                "Unknown verbosity level: {} (quiet|normal|verbose)",
                args[0]
            );
        }
    }
}

async fn show_history(agent: &Agent, args: &[&str]) -> Result<(), CliError> {
    let limit: usize = args.first().and_then(|s| s.parse().ok()).unwrap_or(10);

    let history = agent.get_session_history(limit).await?;

    if history.is_empty() {
        println!("\nNo conversation history yet.\n");
    } else {
        println!("\n📜 Conversation History (last {}):\n", limit);
        for (idx, msg) in history.iter().enumerate() {
            let role = match msg.role {
                mixtape_core::MessageRole::User => "User",
                mixtape_core::MessageRole::Assistant => "Assistant",
                mixtape_core::MessageRole::System => "System",
            };

            let content = if msg.content.len() > 100 {
                format!("{}...", &msg.content[..100])
            } else {
                msg.content.clone()
            };

            if msg.role == mixtape_core::MessageRole::User {
                println!("{}", user_input_margin_line());
                println!(
                    "{}",
                    user_input_line(&format!("{}. {}: {}", idx + 1, role, content))
                );
                println!("{}", user_input_margin_line());
            } else {
                println!("{}. {}: {}", idx + 1, role, content);
            }
        }
        println!();
    }

    Ok(())
}

fn user_input_margin_line() -> &'static str {
    "\x1b[48;5;236m\x1b[2K\x1b[0m"
}

fn user_input_line(text: &str) -> String {
    format!("\x1b[48;5;236m  {}{}\x1b[0m", text, "\x1b[0K")
}

async fn show_session_info(agent: &Agent) -> Result<(), CliError> {
    let usage = agent.get_context_usage();

    println!("\n📊 Session Info:\n");

    if let Some(info) = agent.get_session_info().await? {
        let short_id = &info.id[..8.min(info.id.len())];
        println!("  Session:  {}", short_id);
        println!("  Messages: {}", info.message_count);
    } else {
        println!("  Session:  (memory only)");
        println!("  Messages: {}", usage.total_messages);
    }

    println!(
        "  Context:  {:.1}k / {}k tokens ({}%)",
        usage.context_tokens as f64 / 1000.0,
        usage.max_context_tokens / 1000,
        (usage.usage_percentage * 100.0) as u32
    );
    println!();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    mod verbosity_parse_tests {
        use super::*;

        #[test]
        fn parses_quiet() {
            assert_eq!(Verbosity::parse("quiet"), Some(Verbosity::Quiet));
        }

        #[test]
        fn parses_normal() {
            assert_eq!(Verbosity::parse("normal"), Some(Verbosity::Normal));
        }

        #[test]
        fn parses_verbose() {
            assert_eq!(Verbosity::parse("verbose"), Some(Verbosity::Verbose));
        }

        #[test]
        fn rejects_invalid() {
            assert_eq!(Verbosity::parse("invalid"), None);
            assert_eq!(Verbosity::parse("QUIET"), None); // case sensitive
            assert_eq!(Verbosity::parse(""), None);
            assert_eq!(Verbosity::parse("q"), None);
        }
    }

    mod command_type_parse_tests {
        use super::*;

        #[test]
        fn shell_command() {
            let cmd = CommandType::parse("!ls -la");
            assert_eq!(cmd, CommandType::Shell("ls -la"));
        }

        #[test]
        fn shell_command_empty() {
            let cmd = CommandType::parse("!");
            assert_eq!(cmd, CommandType::Shell(""));
        }

        #[test]
        fn slash_command_no_args() {
            let cmd = CommandType::parse("/help");
            assert_eq!(
                cmd,
                CommandType::Slash {
                    command: "/help",
                    args: vec![]
                }
            );
        }

        #[test]
        fn slash_command_with_args() {
            let cmd = CommandType::parse("/verbosity quiet");
            assert_eq!(
                cmd,
                CommandType::Slash {
                    command: "/verbosity",
                    args: vec!["quiet"]
                }
            );
        }

        #[test]
        fn slash_command_multiple_args() {
            let cmd = CommandType::parse("/history 10 20");
            assert_eq!(
                cmd,
                CommandType::Slash {
                    command: "/history",
                    args: vec!["10", "20"]
                }
            );
        }

        #[test]
        fn regular_input() {
            let cmd = CommandType::parse("hello world");
            assert_eq!(cmd, CommandType::Regular);
        }

        #[test]
        fn regular_input_empty() {
            let cmd = CommandType::parse("");
            assert_eq!(cmd, CommandType::Regular);
        }

        #[test]
        fn regular_input_with_slash_in_middle() {
            let cmd = CommandType::parse("path/to/file");
            assert_eq!(cmd, CommandType::Regular);
        }

        #[test]
        fn regular_input_with_exclamation_in_middle() {
            let cmd = CommandType::parse("hello! world");
            assert_eq!(cmd, CommandType::Regular);
        }
    }

    mod user_input_formatting_tests {
        use super::*;

        #[test]
        fn margin_line_has_ansi_codes() {
            let line = user_input_margin_line();
            assert!(line.contains("\x1b[48;5;236m"));
            assert!(line.contains("\x1b[2K"));
        }

        #[test]
        fn input_line_wraps_text() {
            let line = user_input_line("hello");
            assert!(line.contains("hello"));
            assert!(line.starts_with("\x1b[48;5;236m"));
            assert!(line.ends_with("\x1b[0m"));
        }
    }

    mod help_text_tests {
        use super::*;

        #[test]
        fn header_has_emoji() {
            assert!(help::HEADER.contains("📖"));
        }

        #[test]
        fn shell_commands_documents_bang_syntax() {
            assert!(help::SHELL_COMMANDS.contains("!<command>"));
            assert!(help::SHELL_COMMANDS.contains("!ls"));
        }

        #[test]
        fn navigation_lists_all_slash_commands() {
            assert!(help::NAVIGATION.contains("/help"));
            assert!(help::NAVIGATION.contains("/tools"));
            assert!(help::NAVIGATION.contains("/history"));
            assert!(help::NAVIGATION.contains("/clear"));
            assert!(help::NAVIGATION.contains("/verbosity"));
        }

        #[test]
        fn session_documents_session_command() {
            assert!(help::SESSION.contains("/session"));
        }

        #[test]
        fn exit_documents_exit_commands() {
            assert!(help::EXIT.contains("/exit"));
            assert!(help::EXIT.contains("/quit"));
            assert!(help::EXIT.contains("Ctrl+C"));
            assert!(help::EXIT.contains("Ctrl+D"));
        }

        #[test]
        fn keyboard_documents_shortcuts() {
            assert!(help::KEYBOARD.contains("Up/Down"));
            assert!(help::KEYBOARD.contains("Ctrl+R"));
        }

        #[test]
        fn full_text_contains_all_sections() {
            let full = help::full_text();
            assert!(full.contains("📖"));
            assert!(full.contains("!<command>"));
            assert!(full.contains("/help"));
            assert!(full.contains("/session"));
            assert!(full.contains("/exit"));
            assert!(full.contains("Up/Down"));
        }
    }

    mod format_tool_list_tests {
        use super::*;

        #[test]
        fn empty_list_shows_no_tools_message() {
            let output = format_tool_list(&[]);
            assert!(output.contains("No tools configured"));
        }

        #[test]
        fn single_tool_formatted() {
            let tools = vec![ToolDisplay {
                name: "read_file".to_string(),
                description: "Read a file".to_string(),
            }];
            let output = format_tool_list(&tools);
            assert!(output.contains("read_file - Read a file"));
        }

        #[test]
        fn multiple_tools_formatted() {
            let tools = vec![
                ToolDisplay {
                    name: "read_file".to_string(),
                    description: "Read a file".to_string(),
                },
                ToolDisplay {
                    name: "write_file".to_string(),
                    description: "Write a file".to_string(),
                },
            ];
            let output = format_tool_list(&tools);
            assert!(output.contains("read_file - Read a file"));
            assert!(output.contains("write_file - Write a file"));
        }

        #[test]
        fn header_has_emoji() {
            let output = format_tool_list(&[]);
            assert!(output.contains("🔧"));
            assert!(output.contains("Available Tools"));
        }

        #[test]
        fn tools_are_indented() {
            let tools = vec![ToolDisplay {
                name: "test".to_string(),
                description: "Test tool".to_string(),
            }];
            let output = format_tool_list(&tools);
            assert!(output.contains("  test"));
        }
    }
}
