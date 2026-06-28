# Changelog

## v0.1.0 — Initial Development Release

### Build Status
| Check | Result |
|-------|--------|
| cargo test | 94 passed, 0 failed |
| cargo clippy | 0 warnings |
| tsc -b | 0 errors |
| vitest run | 15 passed, 0 failed |
| vite build | passes |

### Added

#### §3 Multi-Protocol Adapters
- **OpenAI adapter** — SSE streaming with tool call parsing, cancel token, usage tracking
- **Anthropic adapter** (~390 lines) — Messages API SSE streaming, content_block events, tool_use accumulation
- **Gemini adapter** (~300 lines) — generateContent SSE streaming, functionCall parsing
- `ProviderAdapter` trait with `Arc<AtomicBool>` cancel token + `send_with_retry()` (429/5xx exponential backoff)

#### §6 Agent Loop Engine
- `AgentState` discriminated union: idle → thinking → tool_call → error → done
- Multi-turn tool execution loop with `MAX_TOOL_ITERATIONS = 25`
- `onPermissionRequest` + `onStateChange` + `onTokens` callbacks
- Cancellation support

#### §7 Tool System
**Rust** (`tools/mod.rs`, ~620 lines):
- `tool_read_file` — workspace-restricted file reading
- `tool_write_file` — workspace-restricted file writing
- `tool_edit_file` — exact string replacement in files
- `tool_list_dir` — directory listing with stats
- `tool_run_command` — subprocess execution, 30s timeout, process kill
- `tool_grep` — regex pattern search
- `tool_glob` — fnmatch file search with bracket ranges

**TypeScript** (`agent/tools.ts`):
- `ToolDefinition`, `ToolExecutor`, `PermissionDecision` types
- `BASE_TOOLS` (7 tools with JSON Schema parameters)
- `hasSideEffect()` safety classification

#### §8 Frontend UI/UX
- **MarkdownRenderer** — react-markdown + remark-gfm + rehype-highlight (syntax highlighting with Copy button)
- **DiffRenderer** — unified diff parser, green/red lines, line numbers
- **CodeEditor** — tab-based workspace, highlight.js language detection, line numbers gutter
- **TitleBar** — project name + provider switch dropdown
- **FileTree** — collapsible directory tree, double-click to open files
- **PermissionDialog** — Deny / Approve Once / Always Allow per tool

#### §9 Token Tracking
- `TokenUsage` struct: prompt_tokens, completion_tokens, total_tokens
- Session extended with `provider`, `model`, `status`, `tokens`
- SSE parser extracts usage from API response

#### §10 Error Handling
- `AppError::RateLimited` with Retry-After support
- `send_with_retry()`: max 3 retries, exponential backoff (1s/2s/4s)
- Applied to all 3 adapters (OpenAI, Anthropic, Gemini)
- Frontend: consecutive error counter → provider switch suggestion

#### §11 Production Logging
- Removed `cfg!(debug_assertions)` guard from log plugin
- Token usage logging in SSE parser spawn loop

#### §14 Security
- `resolve_workspace_path()` — canonical path validation + symlink escape prevention
- Applied to all 7 file-access tools
- `tool_run_command` — 30s timeout with process termination

#### §16 SQLite Persistence
- `SessionStore` wrapping `rusqlite::Connection`
- Schema: `sessions` + `messages` tables with foreign keys
- Auto-persist on `sessionStore` mutations (fire-and-forget pattern)
- Load sessions on app mount

#### §17 CI/CD
- `.github/workflows/ci.yml` pipeline:
  - `lint` — clippy + tsc
  - `unit-test` — cargo test + vitest
  - `build` — cargo build + vite build
  - `integration` — smoke-test.sh (on main push)
  - `release` — triggered on tag push

#### §18 Rust Doc Comments
- Module-level `//!` documentation on all public APIs
- Covers: lib, commands, keychain, error, stream, sessions, tools, all providers

#### §15 Performance Baseline
- `scripts/perf-bench.sh` — measures debug/release binary size, cold build time

### Fixed

- **§1 Cancel token**: `Arc<AtomicBool>` added to `ProviderAdapter::stream_chat()`, cancels spawned SSE parser on channel drop
- **§2 True streaming**: Rewrote OpenAI adapter from buffer-collect to `bytes_stream()` + `mpsc::unbounded()` + `tokio::spawn`
- **§2 Input validation**: `validate_request()` checks empty messages, max 200 messages, max 32KB per message, empty model/role
- **§4 Error recovery**: `AgentLoop.onDone()` always fires (receivedDone flag), `ChatWindow.finalizePartial()` saves interrupted content as `[interrupted]` message
- **§8 ARIA**: `role="navigation"`, `role="log"`, `aria-live="polite"`, `aria-label` on sidebar/messages/main

### Statistics

- **New files**: 15
- **Modified files**: 18
- **Total changes**: 79 files, 19,772 insertions
- **Rust tests**: 94
- **TypeScript tests**: 15
