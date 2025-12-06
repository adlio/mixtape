# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

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

[Unreleased]: https://github.com/adlio/mixtape/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/adlio/mixtape/releases/tag/v0.1.1
