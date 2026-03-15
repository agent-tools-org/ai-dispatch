# ai-dispatch (aid)

## Install

- NEVER cp binary to `/opt/homebrew/bin/` — macOS provenance xattr blocks execution
- `/opt/homebrew/bin/aid` is a symlink to `~/.cargo/bin/aid`
- Install command (MUST strip provenance first, cargo builds under sandbox inherit it):
  ```bash
  xattr -d com.apple.provenance "$CARGO_TARGET_DIR/release/aid" 2>/dev/null; cp "$CARGO_TARGET_DIR/release/aid" ~/.cargo/bin/aid
  ```
