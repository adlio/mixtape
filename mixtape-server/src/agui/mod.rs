//! AG-UI protocol support for CopilotKit integration.
//!
//! This module provides SSE streaming endpoints that implement the AG-UI protocol,
//! enabling integration with CopilotKit and other AG-UI compatible frontends.
//!
//! # Overview
//!
//! AG-UI (Agent-User Interaction) is an open, event-based protocol that standardizes
//! how AI agents connect to user-facing applications. It uses Server-Sent Events (SSE)
//! to stream agent events to the frontend in real-time.
//!
//! # Event Mapping
//!
//! Mixtape's `AgentEvent`s are mapped to AG-UI events:
//!
//! | AgentEvent | AG-UI Event(s) |
//! |------------|----------------|
//! | `RunStarted` | `RUN_STARTED` |
//! | `RunCompleted` | `TEXT_MESSAGE_END`, `RUN_FINISHED` |
//! | `RunFailed` | `RUN_ERROR` |
//! | `ModelCallStarted` | `TEXT_MESSAGE_START` |
//! | `ModelCallStreaming` | `TEXT_MESSAGE_CONTENT` |
//! | `ToolRequested` | `TOOL_CALL_START`, `TOOL_CALL_ARGS`, `TOOL_CALL_END` |
//! | `ToolCompleted` | `TOOL_CALL_RESULT` |
//! | `ToolFailed` | `TOOL_CALL_RESULT` (with error) |
//! | `PermissionRequired` | `INTERRUPT` |

pub mod convert;
pub mod events;
pub mod handler;
