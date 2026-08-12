Provider marker verification completed.

- `nvidia/<model>` derives group `nvidia`.
- Writes `rate-limit-opencode--nvidia`.
- Reads the same key via `is_group_rate_limited`.
- Unknown attribution remains agent-wide with `provider: unknown`.

Verification:

```text
passed: 4 passed, 0 failed, 0 ignored; command: cargo test --bin aid credibility
```

`aid build` also passed with 0 errors. Report committed as `53df288d`.