# Shared Cargo Cache Measurements

Measured on `ai-dispatch` from `/Users/mingsun/.aid/worktrees/ai-dispatch-6b31c929/feat/shared-cargo-cache`.
Command under test: `cargo check --all-targets`.

## Required Runs

| Run | Environment | Fresh target dir | Wall clock | Notes |
| --- | --- | --- | ---: | --- |
| Cold baseline | `env -u RUSTC_WRAPPER CARGO_TARGET_DIR=<A>` | `/tmp/aid-shared-cache-A.*` | 88.31s | Full fresh target build. |
| sccache | `RUSTC_WRAPPER=sccache CARGO_TARGET_DIR=<B>` after `sccache --zero-stats` | `/tmp/aid-shared-cache-B.*` | 60.47s | Low hit rate; see stats below. |
| Clone seed | `cp -Rc <A> <C>`, then `env -u RUSTC_WRAPPER CARGO_TARGET_DIR=<C>` | `/tmp/aid-shared-cache-C.*` | 12.97s | Clone copy itself took 0.83s. |

## sccache Stats

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

APFS clone seeding pays clearly on this repo: a clone-seeded target checked in 12.97s versus 88.31s cold. The implementation keeps clone seeding and uses `RUSTC_WRAPPER=sccache` when available and not explicitly overridden, but does not force `CARGO_INCREMENTAL=0`.
