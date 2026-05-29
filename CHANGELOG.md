# Changelog

All notable changes to dirge are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and the project
adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

### Added
- Unified permission/authorization engine (single Policy Decision Point):
  op-based rules, `/why` decision-trace command, atomic multi-claim bash.
- Input box scrolls to keep the cursor visible past the height cap, and
  Up/Down navigate across soft-wrapped display rows.

### Fixed
- Secrets in tool output are redacted before reaching the LLM / session
  transcript.
- Transient LLM connection failures ("error sending request") now retry
  with exponential backoff.
- Questionnaire custom answers soft-wrap instead of running off-screen.

### Packaging
- Published to crates.io as the **`dirge-agent`** crate (the short
  `dirge` name was taken); the installed binary is still `dirge`:
  `cargo install dirge-agent`.

## [1.0.0]

First tagged release. dirge is a minimalistic, memory-efficient coding
agent in Rust with:

- A terminal UI with markdown rendering, scrollback, and an info panel.
- Configurable permission modes (standard / restrictive / accept / yolo)
  with op-based rules and session allowlists.
- Tree-sitter bash permission parsing and semantic code tools for
  TypeScript, Python, Clojure, Go, Ruby, Rust, Java, C, and C++.
- Claude-compatible skills, persistent project memory, subagents, MCP and
  LSP integration, and a Janet plugin system.
- Session save/load/resume with LLM-summarization compaction.

[Unreleased]: https://github.com/dirge-code/dirge/compare/v1.0.0...HEAD
[1.0.0]: https://github.com/dirge-code/dirge/releases/tag/v1.0.0
