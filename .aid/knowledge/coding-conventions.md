# Coding Conventions

## File Structure
- Every source file starts with a 2-4 line comment: purpose, exports, deps
- Max 300 lines per file. Split when approaching limit (e.g., `run.rs` → `run_agent.rs` + `run_bestof.rs`)
- Organize by feature, not layer. Each cmd handler is one file in `src/cmd/`

## Rust Patterns
- Use `anyhow::Result` for error handling, `bail!` for early returns
- `Arc<Store>` passed to async handlers; `&Store` for sync
- `#[derive(Debug, Clone, Deserialize)]` for config structs, `#[serde(default)]` on optional fields
- Thread-safe test isolation: use `thread_local!` for test state, never global `Mutex` + `env::set_var`
- No `unwrap()` in production code (project rule); `unwrap()` OK in tests

## Dependencies
- Never use `features = ["full"]` — specify only needed features
- Prefer `std` over external crates for simple cases
- SQLite via `rusqlite` with `busy_timeout=5000`

## Testing
- E2E tests in `tests/` directory, unit tests in `#[cfg(test)] mod tests` blocks
- Use `tempfile::TempDir` for filesystem tests
- `AidHomeGuard` with `thread_local` for test isolation (not global Mutex)
- Test naming: `snake_case` describing the behavior, e.g., `create_worktree_reuses_existing_branch_worktree`

## CLI Pattern
- Commands defined in `cli.rs` as clap derive enums
- Subcommand actions in `cli_actions.rs`
- Handler functions in `cmd/<name>.rs` with internal action enum
- Main dispatch in `main.rs` maps CLI variants → handler calls

## Adding a New Command
1. Add variant to `Commands` enum in `cli.rs`
2. Add subcommand enum to `cli_actions.rs` if needed
3. Create `src/cmd/<name>.rs` with handler function
4. Add `pub mod <name>;` to `src/cmd/mod.rs`
5. Wire dispatch in `main.rs`
