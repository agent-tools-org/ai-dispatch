# Build & Release Process

## Build
- `CARGO_TARGET_DIR` is set per-session to `/tmp/cc-target-{project}-{seq}` — never use `target/release/`
- `cargo build --release` then install:
  ```bash
  cp "$CARGO_TARGET_DIR/release/aid" ~/.cargo/bin/aid && codesign --force --sign - ~/.cargo/bin/aid
  ```
- Must re-sign after copy — macOS sandbox provenance blocks unsigned binaries

## Release Checklist
1. Bump version in `Cargo.toml`
2. Commit: `release: v{version} — {summary}`
3. `git push origin main`
4. `git tag v{version} && git push origin v{version}` (tag MUST be pushed with or right after commit)
5. `cargo publish` (to crates.io)
6. Install locally and verify: `aid --version`
7. Update website: edit `website/src/index.ts` VERSION constant, `cd website && wrangler deploy`
8. Update README.md version badge

## Website
- Source: `website/src/index.ts` (Cloudflare Worker)
- Deploy: `cd website && wrangler deploy`
- Serves: HTML landing page, /llms.txt, /llms-full.txt, /install.sh, /api/*
- VERSION constant controls install.sh echo and /api/info

## Never
- Never cp to `/opt/homebrew/bin/` — macOS xattr blocks execution
- `/opt/homebrew/bin/aid` is a symlink to `~/.cargo/bin/aid`
- Never skip the codesign step
