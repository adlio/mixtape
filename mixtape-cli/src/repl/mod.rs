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

use mixtape_core::{Agent, AgentError, AgentEvent, AgentResponse, AuthorizationResponse};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

/// Permission request data: (proposal_id, tool_name, params_hash, params)
type PermissionData = (String, String, String, Value);

pub use approval::{
    print_confirmation, prompt_for_approval, read_input, ApprovalPrompter, DefaultPrompter,
    PermissionRequest, SimplePrompter,
};
pub use commands::Verbosity;
pub use presentation::{
    indent_lines, new_event_queue, print_result_separator, print_tool_footer, print_tool_header,
    EventPresenter, PresentationHook,
};

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

    // Event queue for tool presentation (allows controlled output timing)
    let event_queue = new_event_queue();

    // Add presentation hook that queues events
    agent.add_hook(PresentationHook::new(Arc::clone(&event_queue)));

    // Presenter for formatting and printing queued events
    let verbosity = Arc::new(Mutex::new(Verbosity::Normal));
    let presenter = EventPresenter::new(
        Arc::clone(&agent),
        Arc::clone(&verbosity),
        Arc::clone(&event_queue),
    );

    // Set up permission handling channel (once, for entire session)
    let (perm_tx, perm_rx) = mpsc::unbounded_channel::<PermissionData>();
    let perm_rx = Arc::new(tokio::sync::Mutex::new(perm_rx));
    agent.add_hook(move |event: &AgentEvent| {
        if let AgentEvent::PermissionRequired {
            proposal_id,
            tool_name,
            params_hash,
            params,
            ..
        } = event
        {
            let _ = perm_tx.send((
                proposal_id.clone(),
                tool_name.clone(),
                params_hash.clone(),
                params.clone(),
            ));
        }
    });

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

                // Run agent with permission handling
                let result = run_with_permissions(
                    Arc::clone(&agent),
                    line.to_string(),
                    spinner,
                    Arc::clone(&perm_rx),
                    &presenter,
                )
                .await;

                match result {
                    Ok(response) => {
                        println!("\n{}\n", response);
                        update_status_line(&agent);
                    }
                    Err(e) => {
                        eprintln!("❌ Error: {}\n", e);
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

/// Run agent with interactive permission handling
async fn run_with_permissions<F: formatter::ToolFormatter>(
    agent: Arc<Agent>,
    input: String,
    spinner: Spinner,
    perm_rx: Arc<tokio::sync::Mutex<mpsc::UnboundedReceiver<PermissionData>>>,
    presenter: &EventPresenter<F>,
) -> Result<AgentResponse, AgentError> {
    // Spawn agent run in background
    let agent_clone = Arc::clone(&agent);
    let mut handle = tokio::spawn(async move { agent_clone.run(&input).await });

    // Lock the receiver for this run
    let mut rx = perm_rx.lock().await;

    // Track if spinner is still active
    let mut spinner = Some(spinner);

    // Wait for permission requests or agent completion
    loop {
        tokio::select! {
            biased;  // Always check permission requests first

            // Check for permission requests
            Some((proposal_id, tool_name, params_hash, params)) = rx.recv() => {
                // Stop spinner before prompting for input
                if let Some(s) = spinner.take() {
                    s.stop().await;
                }

                // Print any queued output before showing the prompt
                presenter.flush();

                // Format tool input for display in approval prompt
                let formatted_display =
                    agent.format_tool_input(&tool_name, &params, mixtape_core::Display::Cli);

                let request = PermissionRequest {
                    tool_name: tool_name.clone(),
                    tool_use_id: proposal_id.clone(),
                    params_hash: params_hash.clone(),
                    formatted_display,
                };

                let response = approval::prompt_for_approval(&request);

                match response {
                    AuthorizationResponse::Once => {
                        agent.authorize_once(&proposal_id).await.ok();
                    }
                    AuthorizationResponse::Trust { grant } => {
                        agent
                            .respond_to_authorization(
                                &proposal_id,
                                AuthorizationResponse::Trust { grant },
                            )
                            .await
                            .ok();
                    }
                    AuthorizationResponse::Deny { reason } => {
                        agent.deny_authorization(&proposal_id, reason).await.ok();
                    }
                }

                // Restart spinner after handling permission
                spinner = Some(Spinner::new("thinking"));
            }

            // Agent finished
            result = &mut handle => {
                // Stop spinner if still running
                if let Some(s) = spinner.take() {
                    s.stop().await;
                }
                // Print any remaining queued output
                presenter.flush();
                return result.unwrap_or_else(|e| Err(AgentError::Tool(e.to_string().into())));
            }
        }
    }
}
