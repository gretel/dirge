# Dirge Usability Improvement Plan

All UX choices default to Claude Code precedent unless noted.

---

## Branch 1: `feature/keyboard-navigation`

**Source:** `src/ui/input.rs`

### Features
1. **Ctrl+A / Ctrl+E** — Start/end of line (supplement Home/End)
2. **Ctrl+B / Ctrl+F** — Char left/right (supplement arrow keys)
3. **Option/Meta+Left → prevWord()** — Skip to previous word start
4. **Option/Meta+Right → nextWord()** — Skip to next word start
5. **Meta+B / Meta+F** — Word skip via Emacs-style chords
6. **Ctrl+K** — Kill to line end (delete + push to kill ring)
7. **Ctrl+U** — Kill to line start (delete + push to kill ring)
8. **Ctrl+W** — Kill word before cursor
9. **Option/Meta+Backspace** — Delete word before (same underlying logic as Ctrl+W)
10. **Option/Meta+D** — Delete word after cursor
11. **Ctrl+Y** — Yank (paste) last kill from kill ring
12. **Meta+Y** — Yank-pop (cycle kill ring after yank)
13. **Ctrl+N / Ctrl+P** — History up/down (supplement arrow keys)

### Data structure additions (`InputEditor`)
```rust
kill_ring: Vec<CompactString>,    // max 10 entries
last_action_was_kill: bool,
last_yank_index: Option<usize>,
```

### Kill ring semantics (match Claude Code)
- Consecutive kills accumulate into one ring entry
- Any non-kill action resets accumulation
- Ctrl+Y inserts most recent kill at cursor, remembers position
- Meta+Y replaces the yanked text with next ring entry
- Ring size capped at 10

### Word boundary logic
Use Unicode-aware word segmentation via `unicode-segmentation` crate (or simple whitespace/punctuation boundaries matching Claude Code's `isVimWordChar` / `isVimWhitespace` / `isVimPunctuation` pattern):

```
word_char: Letter | Number | Mark | _
whitespace: \s
punctuation: everything else
```

prevWord: scan backward to find previous word start boundary
nextWord: scan forward to find next word start boundary

### Tests (`src/tests/input_tests.rs`)
- Ctrl+A/E moves cursor to 0 / buffer.len()
- Ctrl+B/F moves cursor left/right by one char
- Option+Left/Right skips words correctly
- Ctrl+K deletes to end of line and pushes to kill ring
- Ctrl+U deletes to start of line and pushes to kill ring
- Ctrl+W deletes previous word and pushes to kill ring
- Consecutive Ctrl+K appends to same ring entry
- Ctrl+Y inserts most recent kill at cursor
- Meta+Y cycles through kill ring entries
- Ctrl+N/P navigate history
- Word boundary edge cases: start of line, end of line, only whitespace

---

## Branch 2: `feature/tool-display`

**Source:** `src/ui/mod.rs`, `src/agent/tools/edit.rs`

### Features
1. **Tool results shown by default** — Flip `show_tool_details` default to `true` (configurable)
2. **Collapsible tool output** — Truncate large tool results (>500 chars) with `[N more chars]` indicator; expandable via `/details` or config
3. **Colorized diff for `edit` tool** — When edit replaces text, render a unified diff with:
   - `+` lines in green
   - `-` lines in red
   - File header in cyan
   - Show 3 lines of context

### Config additions
```json
{
  "ui": {
    "show_tool_results": true,
    "tool_result_max_chars": 500,
    "show_edit_diff": true
  }
}
```

### Tests
- Tool results render inline when `show_tool_results: true`
- Results > 500 chars are truncated with count indicator
- Edit diff renders green/red/cyan correctly
- Config flags control all three behaviors

---

## Branch 3: `feature/concurrent-tools`

**Source:** `src/agent/runner.rs`, `src/agent/tools/` (new orchestration module)

### Features
1. **Concurrent read-only tool execution** — When the model issues N consecutive read-only tool calls (`read`, `grep`, `find_files`, `list_dir`, `list_symbols`, `get_symbol_body`, `find_definition`, `find_callers`, `find_callees`), execute them concurrently via `tokio::join!` / `FuturesUnordered`
2. **Write tool serialization** — Write tools (`write`, `edit`, `bash`, `task`) always run sequentially in order

### Implementation
- New module: `src/agent/tools/orchestration.rs`
- `partition_tool_calls(tool_calls) -> Vec<Vec<ToolCall>>` — groups consecutive read-only calls into batches, isolates writes
- Read-only flag per tool (add `is_read_only()` to tool definitions)
- Batches yield results in original order
- Streaming tool events interleave: show `◈ tool_a` `◈ tool_b` then results in order

### Tests
- Two concurrent reads complete in parallel (use barrier/sleep to verify)
- A write call between reads forces two separate batches
- Three reads → one batch, one write, two reads → three batches
- Results maintain original tool call order

---

## Branch 4: `feature/multi-line-input`

**Source:** `src/ui/input.rs`, `src/ui/renderer.rs`

### Features
1. **Multi-line input** — `Meta+Enter` or `Shift+Enter` inserts newline
2. **Backslash+Enter** inserts newline (matches Claude Code)
3. **Up/Down arrows** navigate visual lines within input when multiline
4. **History** stores/restores multi-line entries correctly

### Implementation
- `InputEditor` buffer becomes multi-line aware
- Cursor tracks (line, column) in addition to byte offset
- Renderer allocates 1+ lines for input area instead of fixed 1 row
- Input area dynamically expands up to 40% of terminal height
- `handle_key` dispatches Enter modifiers: plain → submit, Meta/Shift → newline

### Tests
- Meta+Enter inserts \n at cursor
- Backslash+Enter inserts \n
- Plain Enter submits
- Up/Down move between visual lines in multi-line input
- History saves and restores multi-line entries

---

## Branch 5: `feature/error-recovery`

**Source:** `src/agent/runner.rs`, `src/provider.rs`

### Features
1. **Auto-compact on context-length error** — When API returns "prompt is too long" or similar, trigger compaction and retry
2. **Max-output-token retry** — When model stops mid-response with `finish_reason: length`, automatically send a "continue" follow-up
3. **API retry with backoff** — Retry transient network errors (5xx, connection refused, timeout) with exponential backoff (1s → 2s → 4s → 8s, max 3 retries)

### Implementation
- `src/agent/recovery.rs` — new module
- `recover_from_error(error, agent, session) -> Result<AgentEvent, RecoverError>` 
- Context-length → call `handle_compress()`, rebuild agent, retry current prompt
- Max-output → send "Please continue from where you left off." as follow-up
- Network → tokio::time::sleep with backoff, retry API call
- Maximum 3 recovery attempts per turn

### Tests
- Mock API returning 400 "context_length_exceeded" → compaction triggered → retry succeeds
- Mock API returning `finish_reason: length` → continue prompt sent
- Mock API failing 3 times → error surfaced on 4th attempt
- Non-recoverable error (401 auth) → immediately surfaced

---

## Branch 6: `feature/input-polish`

**Source:** `src/ui/input.rs`, `src/ui/mod.rs`, `src/ui/markdown.rs`

### Features
1. **Token counter in status bar** — Show estimated tokens for current input text next to session tokens
2. **History search (Ctrl+R)** — Reverse-i-search through input history
3. **Syntax highlighting in code blocks** — Apply language-aware coloring to fenced code blocks in rendered output
4. **Copy code block (Ctrl+Shift+C)** — When cursor is over a code block in output, copy its contents to clipboard

### Implementation
- Token counter: `Session::estimate_tokens()` on input buffer, display as `input: N tk` in status
- History search: Ctrl+R activates search mini-buffer, incremental filtering, Enter accepts, Esc cancels
- Syntax highlighting: Use `syntect` crate for language detection and coloring within markdown code fences
- Copy block: Track code block boundaries in rendered output; when Ctrl+Shift+C pressed, find enclosing block and copy

### Tests
- Token counter updates as user types
- History search finds matching entries
- History search Enter accepts, Esc cancels
- Code blocks get syntax-specific coloring
- Copy block copies correct content

---

## Branch 7: `feature/llm-context`

**Source:** `src/agent/tools/read.rs`, `src/ui/mod.rs`, `src/agent/builder.rs`

### Features
1. **Line numbers in read output** — Prefix read output with `LINE: content` format
2. **Workspace tree** — Auto-include basic project structure in system prompt (top-level files + tree up to depth 2)
3. **Environment preamble** — Inject OS, shell, working directory, git branch into system prompt
4. **Error classification** — Distinguish error types in AgentEvent: `ToolFailed`, `PermissionDenied`, `ApiError`, `ModelRefused`
5. **Workspace-aware path handling** — Show relative paths in tool display rather than absolute

### Implementation
- read.rs: Format output as `   1: content\n   2: content\n...` with right-aligned line numbers
- agent/builder.rs: Add workspace tree to system prompt preamble
- System prompt additions:
  ```
  [Environment]
  OS: macOS 14.5
  Shell: zsh
  Workspace: /Users/yogthos/src/dirge
  Branch: main
  
  [Project Structure]
  src/
    agent/
    ui/
    ...
  ```
- AgentEvent enum: split `Error(String)` into structured variants
- Tool display: resolve relative paths for display

### Tests
- Read output includes line numbers
- System prompt contains environment block
- Error classification propagates correctly through event channel
- Tool display shows relative paths

---

## Implementation Order

```
keyboard-navigation → tool-display → concurrent-tools → multi-line-input
  → error-recovery → input-polish → llm-context
```

Rationale: Keyboard nav is most user-facing and immediately impactful. Tool display is critical for LLM usability. Concurrent tools speeds up every multi-tool turn. Multi-line input enables longer prompts. Error recovery builds resilience. Input polish and LLM context are nice-to-haves.
