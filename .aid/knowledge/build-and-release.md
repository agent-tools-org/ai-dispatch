# Build & Release Process

## Build
- Use a task-specific `CARGO_TARGET_DIR` under `/tmp`; never rely on the
  repository's `target/` path.
- Install the verified checkout with:
  ```bash
  CARGO_TARGET_DIR=/tmp/aid-install cargo install --path . --locked --force
  ```
- Verify both the binary and its release-managed skill:
  ```bash
  aid --version
  aid init
  aid config skills
  ```

## Release Checklist
1. Commit the feature or fix and start from a clean worktree.
2. Write curated Markdown release notes containing only `- ` bullets.
3. Run `scripts/release.sh --dry-run <version> <notes-file>`.
4. Run `scripts/release.sh <version> <notes-file>`.
5. Confirm that `main` and `v<version>` were pushed together.
6. Install locally, run `aid init`, and verify `aid --version`.

The release script owns the `Cargo.toml` and `Cargo.lock` version bump,
`CHANGELOG.md` entry, release commit, tag, and pushes. Do not reproduce those
steps manually.

## Official Guide

`default-skills/aid-guide/` is a release artifact and the authoritative
operating guide for the matching AID version.

- Update the relevant guide reference in the same commit as any public command,
  flag, lifecycle, safety invariant, configuration key, or recommended workflow.
- Keep `references/command-index.md` complete.
- `aid init`, `aid setup`, and `aid upgrade` refresh the official guide while
  preserving user-owned skills.
- Validate with:
  ```bash
  python3 ~/.codex/skills/.system/skill-creator/scripts/quick_validate.py \
    default-skills/aid-guide
  cargo test --test aid_guide_e2e --test init_e2e
  ```

## Website
- Source: `website/src/index.ts` (Cloudflare Worker)
- Deploy: `cd website && wrangler deploy`
- Serves: HTML landing page, /llms.txt, /llms-full.txt, /install.sh, /api/*
- VERSION constant controls install.sh echo and /api/info

## Never
- Never copy directly to `/opt/homebrew/bin/`.
- `/opt/homebrew/bin/aid` is a symlink to `~/.cargo/bin/aid`
- Never push a release commit without immediately pushing its matching tag.
- Never ship a public behavior change without updating `aid-guide`.
