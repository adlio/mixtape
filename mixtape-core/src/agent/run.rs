//! The agentic loop - core execution logic for Agent

use std::time::Instant;

use crate::events::AgentEvent;
use crate::types::{Message, StopReason, ToolDefinition};

use super::context::{build_effective_prompt, resolve_context, ContextLoadResult, PathVariables};
use super::helpers::extract_text_response;
use super::types::{AgentError, AgentResponse, TokenUsageStats, ToolCallInfo};
use super::Agent;

#[cfg(feature = "session")]
use crate::session::{MessageRole, Session, SessionMessage, ToolCall, ToolResult};

#[cfg(feature = "session")]
use super::session::convert_session_message_to_mixtape;

impl Agent {
    /// Run the agent with a user message
    ///
    /// This will execute an agentic loop, calling the model and executing tools
    /// until the model returns a final text response.
    ///
    /// Returns an `AgentResponse` containing the text response, tool call history,
    /// token usage statistics, and timing information.
    ///
    /// If a session store is configured, this will automatically load and resume
    /// the session for the current directory.
    ///
    /// # Errors
    ///
    /// Returns `AgentError` which can be:
    /// - `Provider` - API errors (authentication, rate limits, network issues)
    /// - `Tool` - Tool execution failures
    /// - `Session` - Session storage errors (if session feature enabled)
    /// - `NoResponse` - Model returned no text
    /// - `MaxTokensExceeded` - Response hit token limit
    /// - `ContentFiltered` - Response was filtered
    /// - `ToolDenied` - Tool execution was denied by user/policy
    pub async fn run(&self, user_message: &str) -> Result<AgentResponse, AgentError> {
        let run_start = Instant::now();

        // Track execution statistics
        let mut tool_call_infos: Vec<ToolCallInfo> = Vec::new();
        let mut total_input_tokens: usize = 0;
        let mut total_output_tokens: usize = 0;
        let mut model_call_count: usize = 0;

        // Resolve context files at runtime
        let context_result = self.resolve_context_files()?;

        // Store for inspection via last_context_info()
        *self.last_context_result.write() = Some(context_result.clone());

        // Build effective system prompt with context files
        let effective_system_prompt =
            build_effective_prompt(self.system_prompt.as_deref(), &context_result);

        // Emit run started event
        self.emit_event(AgentEvent::RunStarted {
            input: user_message.to_string(),
            timestamp: run_start,
        });

        // Load or create session if session store is configured
        #[cfg(feature = "session")]
        let mut session: Option<Session> = if let Some(store) = &self.session_store {
            let sess = store.get_or_create_session().await?;

            // Hydrate conversation manager from session history
            if !sess.messages.is_empty() {
                let mut messages: Vec<Message> = vec![];
                for msg in &sess.messages {
                    messages.extend(convert_session_message_to_mixtape(msg)?);
                }
                self.conversation_manager.write().hydrate(messages);

                self.emit_event(AgentEvent::SessionResumed {
                    session_id: sess.id.clone(),
                    message_count: sess.messages.len(),
                    created_at: sess.created_at,
                });
            }

            Some(sess)
        } else {
            None
        };

        #[cfg(feature = "session")]
        let mut session_tool_calls: Vec<ToolCall> = Vec::new();
        #[cfg(feature = "session")]
        let mut session_tool_results: Vec<ToolResult> = Vec::new();

        // Add new user message to conversation manager
        self.conversation_manager
            .write()
            .add_message(Message::user(user_message));

        loop {
            // Build tool definitions
            let tool_defs: Vec<ToolDefinition> = self
                .tools
                .iter()
                .map(|t| ToolDefinition {
                    name: t.name().to_string(),
                    description: t.description().to_string(),
                    input_schema: t.input_schema(),
                })
                .collect();

            // Get messages for context from conversation manager
            let limits =
                crate::conversation::ContextLimits::new(self.provider.max_context_tokens());
            let provider = &self.provider;
            let estimate_tokens = |msgs: &[Message]| provider.estimate_message_tokens(msgs);
            let context_messages = self
                .conversation_manager
                .read()
                .messages_for_context(limits, &estimate_tokens);

            // Emit model call started event
            let model_call_start = Instant::now();
            self.emit_event(AgentEvent::ModelCallStarted {
                message_count: context_messages.len(),
                tool_count: tool_defs.len(),
                timestamp: model_call_start,
            });

            // Call the model via provider with streaming
            let response = self
                .generate_with_streaming(
                    context_messages,
                    tool_defs,
                    effective_system_prompt.clone(),
                )
                .await?;

            // Track model call stats
            model_call_count += 1;
            if let Some(ref usage) = response.usage {
                total_input_tokens += usage.input_tokens;
                total_output_tokens += usage.output_tokens;
            }

            // Emit model call completed event
            let response_text = response.message.text();

            self.emit_event(AgentEvent::ModelCallCompleted {
                response_content: response_text,
                tokens: response.usage,
                duration: model_call_start.elapsed(),
                stop_reason: Some(response.stop_reason),
            });

            // Add assistant response to conversation manager
            self.conversation_manager
                .write()
                .add_message(response.message.clone());

            match response.stop_reason {
                StopReason::ToolUse => {
                    let tool_results = self
                        .process_tool_calls(
                            &response.message,
                            &mut tool_call_infos,
                            #[cfg(feature = "session")]
                            &mut session_tool_calls,
                            #[cfg(feature = "session")]
                            &mut session_tool_results,
                        )
                        .await;

                    // Add tool results to conversation manager
                    self.conversation_manager
                        .write()
                        .add_message(Message::tool_results(tool_results));
                }
                StopReason::EndTurn => {
                    return self
                        .finalize_run(
                            &response.message,
                            user_message,
                            tool_call_infos,
                            total_input_tokens,
                            total_output_tokens,
                            model_call_count,
                            run_start,
                            #[cfg(feature = "session")]
                            &mut session,
                            #[cfg(feature = "session")]
                            &session_tool_calls,
                            #[cfg(feature = "session")]
                            &session_tool_results,
                        )
                        .await;
                }
                StopReason::MaxTokens => {
                    self.emit_event(AgentEvent::RunFailed {
                        error: AgentError::MaxTokensExceeded.to_string(),
                        duration: run_start.elapsed(),
                    });
                    return Err(AgentError::MaxTokensExceeded);
                }
                StopReason::ContentFiltered => {
                    self.emit_event(AgentEvent::RunFailed {
                        error: AgentError::ContentFiltered.to_string(),
                        duration: run_start.elapsed(),
                    });
                    return Err(AgentError::ContentFiltered);
                }
                StopReason::StopSequence => {
                    // Treat stop sequence similar to EndTurn - extract text response
                    let final_response =
                        extract_text_response(&response.message).unwrap_or_default();

                    let duration = run_start.elapsed();
                    self.emit_event(AgentEvent::RunCompleted {
                        output: final_response.clone(),
                        duration,
                    });

                    let token_usage = if total_input_tokens > 0 || total_output_tokens > 0 {
                        Some(TokenUsageStats {
                            input_tokens: total_input_tokens,
                            output_tokens: total_output_tokens,
                        })
                    } else {
                        None
                    };

                    return Ok(AgentResponse {
                        text: final_response,
                        tool_calls: tool_call_infos,
                        token_usage,
                        duration,
                        model_calls: model_call_count,
                    });
                }
                StopReason::PauseTurn => {
                    // Extended thinking continuation - the model wants to continue thinking
                    // We continue the loop to allow further turns
                }
                StopReason::Unknown => {
                    let error = AgentError::UnexpectedStopReason("Unknown".to_string());
                    self.emit_event(AgentEvent::RunFailed {
                        error: error.to_string(),
                        duration: run_start.elapsed(),
                    });
                    return Err(error);
                }
            }
        }
    }

    /// Finalize a successful run, saving session if configured
    #[allow(clippy::too_many_arguments)]
    #[allow(unused_variables)] // user_message only used with session feature
    async fn finalize_run(
        &self,
        message: &Message,
        user_message: &str,
        tool_call_infos: Vec<ToolCallInfo>,
        total_input_tokens: usize,
        total_output_tokens: usize,
        model_call_count: usize,
        run_start: Instant,
        #[cfg(feature = "session")] session: &mut Option<Session>,
        #[cfg(feature = "session")] session_tool_calls: &[ToolCall],
        #[cfg(feature = "session")] session_tool_results: &[ToolResult],
    ) -> Result<AgentResponse, AgentError> {
        let final_response = extract_text_response(message).ok_or(AgentError::NoResponse)?;

        // Save session if configured
        #[cfg(feature = "session")]
        if let (Some(ref mut sess), Some(ref store)) = (session, &self.session_store) {
            use chrono::Utc;

            // Add user message to session
            sess.messages.push(SessionMessage {
                role: MessageRole::User,
                content: user_message.to_string(),
                tool_calls: vec![],
                tool_results: vec![],
                timestamp: Utc::now(),
            });

            // Add assistant response to session
            sess.messages.push(SessionMessage {
                role: MessageRole::Assistant,
                content: final_response.clone(),
                tool_calls: session_tool_calls.to_vec(),
                tool_results: session_tool_results.to_vec(),
                timestamp: Utc::now(),
            });

            // Save session
            store.save_session(sess).await?;

            // Emit session saved event
            self.emit_event(AgentEvent::SessionSaved {
                session_id: sess.id.clone(),
                message_count: sess.messages.len(),
            });
        }

        // Emit run completed event
        let duration = run_start.elapsed();
        self.emit_event(AgentEvent::RunCompleted {
            output: final_response.clone(),
            duration,
        });

        // Build token usage stats
        let token_usage = if total_input_tokens > 0 || total_output_tokens > 0 {
            Some(TokenUsageStats {
                input_tokens: total_input_tokens,
                output_tokens: total_output_tokens,
            })
        } else {
            None
        };

        Ok(AgentResponse {
            text: final_response,
            tool_calls: tool_call_infos,
            token_usage,
            duration,
            model_calls: model_call_count,
        })
    }

    /// Resolve context files from configured sources
    fn resolve_context_files(&self) -> Result<ContextLoadResult, AgentError> {
        if self.context_sources.is_empty() {
            return Ok(ContextLoadResult::default());
        }

        let vars = PathVariables::current();
        resolve_context(&self.context_sources, &vars, &self.context_config).map_err(|e| e.into())
    }
}
