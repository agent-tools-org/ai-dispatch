Quota-marker review follow-up committed as `f7a5bcc2`.

- OpenCode groups derive from any provider prefix before `/`, including `nvidia`; no allowlist or model table.
- Named provider refusals use the same extraction.
- Unknown attribution records `provider: unknown` and conservatively holds the whole agent.
- Full messages and usable ISO/RFC3339 reset times are preserved.

Before fix: both `nvidia` attribution tests failed.  
After fix: `aid build` passed; 2,297 tests passed, 0 failed.