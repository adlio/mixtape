//! Interactive REPL for mixtape agents

mod approval;
mod commands;
mod core;
mod formatter;
mod input;
mod presentation;
mod spinner;
mod status;

use crate::error::CliError;
use commands::{handle_special_command, SpecialCommandResult};
use core::{input_prompt, print_input_padding, print_welcome, reset_input_style};
use input::InputStyleHelper;
use rustyline::config::Config;
use rustyline::error::ReadlineError;
use rustyline::{Cmd, Editor, KeyEvent};
use spinner::Spinner;
use status::{clear_status_line, update_status_line};

use mixtape_core::Agent;
use std::sync::{Arc, Mutex};

pub use approval::{
    print_confirmation, print_tool_header, prompt_for_approval, read_input, ApprovalPrompter,
    DefaultPrompter, PermissionRequest, SimplePrompter,
};
pub use commands::Verbosity;
pub use presentation::{indent_lines, PresentationHook};

/// Run an interactive REPL for the agent
///
/// This provides a command-line interface with:
/// - Up/down arrow history
/// - Ctrl+R reverse search
/// - Multi-line input support
/// - Special commands (!shell, /help, etc)
/// - Automatic session management
/// - Rich tool presentation with CLIPresenter formatting
/// - Tool approval prompts (when using Registry approval mode)
///
/// # Errors
///
/// Returns `CliError` which can be:
/// - `Agent` - Agent execution errors
/// - `Session` - Session storage errors
/// - `Readline` - Input/readline errors
/// - `Io` - Filesystem errors (history loading/saving)
///
/// # Example
/// ```ignore
/// use mixtape_core::{Agent, ClaudeSonnet4_5};
/// use mixtape_cli::run_cli;
///
/// let agent = Agent::builder()
///     .bedrock(ClaudeSonnet4_5)
///     .build()
///     .await?;
///
/// run_cli(agent).await?;
/// ```
pub async fn run_cli(agent: Agent) -> Result<(), CliError> {
    let agent = Arc::new(agent);

    // Add presentation hook for rich tool display
    let verbosity = Arc::new(Mutex::new(Verbosity::Normal));
    agent.add_hook(PresentationHook::new(
        Arc::clone(&agent),
        Arc::clone(&verbosity),
    ));
    print_welcome(&agent).await?;

    let config = Config::default();
    let mut rl: Editor<InputStyleHelper, rustyline::history::DefaultHistory> =
        Editor::with_config(config)?;
    rl.set_helper(Some(InputStyleHelper));

    // Bind Ctrl-J to insert newline instead of submitting
    rl.bind_sequence(KeyEvent::ctrl('J'), Cmd::Newline);

    let history_path = dirs::cache_dir()
        .map(|p| p.join("mixtape/history.txt"))
        .unwrap_or_else(|| ".mixtape/history.txt".into());

    // Load history
    if history_path.exists() {
        rl.load_history(&history_path).ok();
    }

    loop {
        // Update persistent status line at bottom of terminal
        update_status_line(&agent);

        print_input_padding();
        let readline = rl.readline(input_prompt());
        reset_input_style();

        match readline {
            Ok(line) => {
                let line = line.trim();

                if line.is_empty() {
                    continue;
                }

                rl.add_history_entry(line)?;

                // Handle special commands
                if let Some(result) = handle_special_command(line, &agent, &verbosity).await? {
                    match result {
                        SpecialCommandResult::Exit => break,
                        SpecialCommandResult::Continue => continue,
                    }
                }

                // Show animated thinking indicator
                println!(); // Move to new line, clearing input background
                let spinner = Spinner::new("thinking");

                // Regular agent interaction
                match agent.run(line).await {
                    Ok(response) => {
                        // Stop spinner and clear the line
                        spinner.stop().await;
                        println!("\n{}\n", response);

                        // Update status line with new context usage
                        update_status_line(&agent);
                    }
                    Err(e) => {
                        // Stop spinner and print error
                        spinner.stop().await;
                        eprintln!("❌ Error: {}\n", e);

                        // Update status line even after error
                        update_status_line(&agent);
                    }
                }
            }
            Err(ReadlineError::Interrupted) => {
                // Ctrl+C - just continue
                println!("^C");
                continue;
            }
            Err(ReadlineError::Eof) => {
                // Ctrl+D - exit
                break;
            }
            Err(err) => {
                eprintln!("Error: {:?}", err);
                break;
            }
        }
    }

    // Clear persistent status line on exit
    clear_status_line();

    // Gracefully shutdown agent (disconnects MCP servers)
    agent.shutdown().await;

    // Save history
    if let Some(parent) = history_path.parent() {
        std::fs::create_dir_all(parent).ok();
    }
    rl.save_history(&history_path)?;

    println!("\n👋 Goodbye!\n");
    Ok(())
}
