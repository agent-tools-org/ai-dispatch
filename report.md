Implemented and committed as `3ab3ea44`.

- OpenCode quota holds are scoped to known providers (`opencode`, `opencode-go`, `mimo`) when attribution is available.
- Unattributed refusals record `provider: unknown` and conservatively hold the whole agent.
- Markers retain the full refusal message.
- Valid ISO/RFC3339 reset timestamps populate `recovery_at`; unknown times remain empty.

Before fix: 2 regression failures, including `recovery_at: None` and truncated messages.  
After fix: `aid build` passes; 2,297 tests passed, 0 failed.