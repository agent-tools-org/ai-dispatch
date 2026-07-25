# Shared Cargo Cache Measurements

Measured on `ai-dispatch` from `/Users/mingsun/.aid/worktrees/ai-dispatch-6b31c929/feat/shared-cargo-cache`.
Command under test: `cargo check --all-targets`.

## Required Runs

| Run | Environment | Fresh target dir | Wall clock | Notes |
| --- | --- | --- | ---: | --- |
| Cold baseline | `env -u RUSTC_WRAPPER CARGO_TARGET_DIR=<root>/cold` | `/tmp/aid-shared-cache-realpath.rEmRjh/cold` | 37.13s | Full fresh target build on a warm machine. |
| Source base | `env -u RUSTC_WRAPPER CARGO_TARGET_DIR=<root>/_base` | `/tmp/aid-shared-cache-realpath.rEmRjh/_base` | 38.01s | Populates the shared source target used by the shipped layout. |
| Real-path clone seed | `CARGO_TARGET_DIR=<root>/_base aid run noop ... --worktree feat/shared-cache-realpath-fixed`, then `CARGO_TARGET_DIR=<root>/feat-shared-cache-realpath-fixed cargo check --all-targets` | `/tmp/aid-shared-cache-realpath.rEmRjh/feat-shared-cache-realpath-fixed` | 8.45s | Worktree setup recorded clone seeding from `_base` in 284ms. |

## Layout

The aid-managed default uses `<root>/_base` for non-worktree Rust builds and `<root>/<sanitized-branch>` for worktree builds. When the user explicitly sets `CARGO_TARGET_DIR`, that value remains the source target directory, and branch targets are created beside it. Branch target dirs are siblings of the source target, not children of it, so a branch target cannot recursively contain another branch target.

The real-path measurement created `<root>/existing-branch` before dispatch. After seeding `feat/shared-cache-realpath-fixed`, a `find` check for nested branch dirs returned 0. A unit test also creates `feat/cache-a`, `feat/cache-b`, and `feat/cache-c` target dirs in sequence and asserts none contains another.

The `aid run noop` dispatch used `HOME` under `/tmp` so the sandbox could create an aid-managed worktree. It used `--verify true` to keep the dispatch focused on setup; the timed `cargo check` was run separately against the seeded target.

## Rejected: sccache

Initial sccache run into fresh dir B:

```text
Compile requests: 177
Compile requests executed: 118
Cache hits: 1
Cache misses: 114
Non-cacheable calls: 55
Cache hits rate: 0.87%
Cache hits rate (Rust): 0.00%
Non-cacheable reasons: crate-type 34, incremental 11, "-" 4, "-o" 4, missing input 2
```

Because that hit rate was not useful, I ran a second fresh target dir after B had populated sccache. It still reported 176 requests, 1 cache hit, 114 misses, and 0.00% Rust hit rate. Setting `CARGO_INCREMENTAL=0` removed `incremental` from the non-cacheable reasons on the replay run, but it did not improve the hit rate: 176 requests, 1 cache hit, 114 misses, and 0.00% Rust hit rate.

Investigation with `cargo check -p ai-dispatch -vv` showed target-dir-specific absolute paths in rustc inputs, including `DYLD_FALLBACK_LIBRARY_PATH`, `--out-dir`, `-L dependency`, `OUT_DIR`, and `--extern` paths. Those path differences explain the repeated Rust misses across fresh target directories better than incremental compilation does. I did not set `CARGO_INCREMENTAL=0` in the implementation because it did not improve cache replay here.

## Decision

APFS clone seeding pays on this repo when it runs through the shipped worktree setup path: the seeded target checked in 8.45s versus 37.13s cold. The implementation keeps clone seeding, records seeded/skipped outcomes as setup events, and removes `RUSTC_WRAPPER=sccache` because the measured Rust hit rate was 0.00%.
