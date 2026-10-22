// Display profiles for built-in agent kinds.
// Keeps CLI names, descriptions, costs, and task hints together.

pub(super) fn profile_qwen() -> (&'static str, &'static str, &'static str, &'static str, bool) {
    (
        "qwen",
        "Research, coding with Qwen3-Coder models",
        "free (OAuth) or Alibaba Cloud subscription",
        "implement, refactor, research, explain",
        true,
    )
}

pub(super) fn profile_codex() -> (&'static str, &'static str, &'static str, &'static str, bool) {
    (
        "codex",
        "Complex implementation, multi-file refactors",
        "$0.10-$8/M blended",
        "implement, create, build, refactor, test",
        true,
    )
}

pub(super) fn profile_copilot() -> (&'static str, &'static str, &'static str, &'static str, bool) {
    (
        "copilot",
        "General coding, repo navigation, tool-assisted implementation",
        "subscription",
        "implement, build, refactor, test, explain, debug",
        true,
    )
}

pub(super) fn profile_open_code() -> (&'static str, &'static str, &'static str, &'static str, bool) {
    (
        "opencode",
        "Simple edits, renames, type annotations",
        "free-$2/M blended",
        "rename, change, update, fix typo, add type",
        true,
    )
}

pub(super) fn profile_command_code() -> (&'static str, &'static str, &'static str, &'static str, bool) {
    (
        "commandcode",
        "General coding via Command Code CLI",
        "commandcode.ai subscription / credits",
        "implement, create, build, refactor, test, explain",
        true,
    )
}

pub(super) fn profile_cursor() -> (&'static str, &'static str, &'static str, &'static str, bool) {
    (
        "cursor",
        "General coding, strong model selection, frontend",
        "$20/mo subscription",
        "implement, create, build, refactor, ui, frontend, css",
        true,
    )
}

pub(super) fn profile_kilo() -> (&'static str, &'static str, &'static str, &'static str, bool) {
    (
        "kilo",
        "Simple edits (free tier)",
        "free",
        "rename, change, update, fix typo, add type",
        true,
    )
}

pub(super) fn profile_mi_mo_code() -> (&'static str, &'static str, &'static str, &'static str, bool) {
    (
        "mimocode",
        "Coding via Xiaomi MiMo Code CLI (opencode-family)",
        "free / key-store",
        "implement, change, update, refactor, add type",
        true,
    )
}

pub(super) fn profile_droid() -> (&'static str, &'static str, &'static str, &'static str, bool) {
    (
        "droid",
        "Complex implementation, multi-agent orchestration",
        "BYOK (API key)",
        "implement, create, build, refactor, test, orchestrate",
        true,
    )
}

pub(super) fn profile_oz() -> (&'static str, &'static str, &'static str, &'static str, bool) {
    (
        "oz",
        "Complex implementation, multi-file refactors",
        "Warp subscription",
        "implement, create, build, refactor, test",
        true,
    )
}

pub(super) fn profile_claude() -> (&'static str, &'static str, &'static str, &'static str, bool) {
    (
        "claude",
        "General coding, review, refactoring, research",
        "$1-$75/M blended",
        "implement, review, refactor, explain, research, test",
        true,
    )
}

pub(super) fn profile_antigravity() -> (&'static str, &'static str, &'static str, &'static str, bool) {
    (
        "agy",
        "Research, coding, web search, file editing (Antigravity CLI — replaces Gemini CLI for \
            Google One / free tier)",
        "free (Google One / Gemini Code Assist) or BYOK",
        "research, explain, implement, create, analyze, build",
        true,
    )
}

pub(super) fn profile_grok() -> (&'static str, &'static str, &'static str, &'static str, bool) {
    (
        "grok",
        "General coding via Grok CLI",
        "grok.com subscription",
        "implement, explain, refactor, research",
        true,
    )
}
