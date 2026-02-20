# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.3.1] - 2026-02-20

### Added

- Claude Sonnet 4.6 model (200K context, 64K output)
- GLM 4.7 model (202K context, 131K output)
- GLM 4.7 Flash model (202K context, 131K output)
- MiniMax M2.1 model (204K context, 131K output)
- Qwen3 Coder Next model (256K context, 65K output)

## [0.3.0] - 2026-02-14

### Added

- **mixtape-server** *(experimental)*: HTTP server with AG-UI protocol support. API surface may change in future releases.
- Claude Opus 4.6 model (flagship, 200K context, 128K output)
- Claude Opus 4.1 model (200K context, 32K output)
- Nova 2 Sonic model (1M context, 65K output)
- Mistral Ministral 3B, 8B, 14B models
- Mistral Pixtral Large model
- Mistral Voxtral Mini 3B and Voxtral Small 24B models (speech+text input)
- Qwen3 32B, Qwen3 Coder 30B, Qwen3 Next 80B, Qwen3 VL 235B models
- Google Gemma 3 12B and 4B models
- DeepSeek V3.2 model
- Kimi K2.5 model
- Inference profile tests for models requiring `InferenceProfile::Global`
- `bedrock_provider_match!` macro in model_verification example to reduce boilerplate

### Changed

- **BREAKING**: Renamed `DeepSeekV3` to `DeepSeekV3_1` to match the model's actual version
- Split `pub use models::{...}` re-exports into per-vendor blocks to prevent `cargo fmt` from mixing vendor groupings
- Reordered Claude model definitions by family and version (Opus 4 → 4.1 → 4.5 → 4.6, then Sonnet, then Haiku)
- `mixtape-anthropic-sdk` now uses `version.workspace = true` instead of a hardcoded version

### Fixed

- Claude 3.7 Sonnet output token limit: 8,192 → 64,000
- Claude Haiku 4.5 output token limit: 8,192 → 64,000
- Claude Opus 4.5 output token limit: 32,000 → 64,000
- Stale `--providers` CLI flag in model_verification example docs (correct flag is `--vendors`)

## [0.2.1] - 2026-01-05

### Fixed

- Use i64 for rusqlite in session store for cross-platform compatibility
- Use i64 for rusqlite COUNT queries for cross-platform compatibility

## [0.2.0] - 2026-01-05

### Added

- Claude Sonnet 4.5 1M model support
- Tool grouping exports for filesystem and process modules
- `builder.add_trusted_tool()` convenience method
- Animated spinner for thinking indicator in CLI
- Improved tool execution event model for approval

### Changed

- Updated non-interactive examples to use `add_trusted_tool()`

## [0.1.1] - 2026-01-04

### Added

- Initial release of the mixtape agent framework
- **mixtape-core**: Core agent framework with conversation management, tool execution, and permission system
  - Support for AWS Bedrock provider (Claude models)
  - Support for Anthropic API provider
  - MCP (Model Context Protocol) client integration
  - Session persistence for conversation history
  - Flexible permission system for tool authorization
- **mixtape-anthropic-sdk**: Minimal Anthropic API client with streaming support
- **mixtape-tools**: Ready-to-use tool implementations
  - Filesystem tools (read, write, glob, grep)
  - Process management (bash execution)
  - Web fetching with robots.txt compliance
  - SQLite database operations
  - AWS SigV4 authenticated requests
- **mixtape-cli**: Session storage and REPL utilities for interactive agents

[Unreleased]: https://github.com/adlio/mixtape/compare/v0.3.1...HEAD
[0.3.1]: https://github.com/adlio/mixtape/compare/v0.3.0...v0.3.1
[0.3.0]: https://github.com/adlio/mixtape/compare/v0.2.1...v0.3.0
[0.2.1]: https://github.com/adlio/mixtape/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/adlio/mixtape/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/adlio/mixtape/releases/tag/v0.1.1
