// Generated documentation coverage from Clap, serde, lifecycle enums, and MCP.
// Keeps existing documentation gaps explicit; pins no incidental guide prose.
// Deps: bundled GUIDE_FILES and the production registries/config types.

use std::{cell::RefCell, collections::BTreeSet};
use clap::CommandFactory;
use serde::Deserialize;
use serde::de::{IntoDeserializer, Visitor};
use serde::de::value::{MapDeserializer, SeqDeserializer};

use super::GUIDE_FILES;

// Existing undocumented flags only. Remove entries as documented; never add gaps.
const UNDOCUMENTED_FLAGS: &[&str] = &[
    "--active",
    "--agent",
    "--agents",
    "--as",
    "--as-of",
    "--async",
    "--auto",
    "--base",
    "--best-of",
    "--brief",
    "--category",
    "--checks",
    "--confidence",
    "--count",
    "--direction",
    "--disable",
    "--enable",
    "--escalate",
    "--eval",
    "--eval-feedback-template",
    "--explain",
    "--files",
    "--finding",
    "--finding-file",
    "--git",
    "--hook",
    "--id",
    "--include-waiting",
    "--insights",
    "--iterate",
    "--judge",
    "--key",
    "--lines",
    "--log",
    "--max-runs",
    "--message",
    "--metric",
    "--name",
    "--no-audit",
    "--no-link-deps",
    "--note",
    "--older-than",
    "--on-done",
    "--package",
    "--parent",
    "--peer-review",
    "--project",
    "--prompt-file",
    "--reset",
    "--running",
    "--session",
    "--source",
    "--stats",
    "--stdin",
    "--stream",
    "--target",
    "--today",
    "--transcript",
    "--type",
    "--update",
    "--valid-from",
    "--verdict",
    "--warnings",
    "--worktrees",
];

// Existing undocumented serde keys only. Remove entries as documented; never add gaps.
const UNDOCUMENTED_CONFIG_KEYS: &[&str] = &[
    "api_key",
    "auto_model",
    "cost_limit_usd",
    "escalate_after_secs",
    "external_cost_usd",
    "external_tasks",
    "external_tokens",
    "free_model",
    "gitbutler",
    "hard_cap_hours",
    "headers",
    "idle_escalate_secs",
    "idle_nudge_secs",
    "idle_timeout",
    "idle_warn_secs",
    "language",
    "max_duration_mins",
    "max_task_duration_mins",
    "notes",
    "nudge_after_secs",
    "on_done",
    "on_failed",
    "prefer_budget",
    "query",
    "research",
    "simple_edit",
    "smart_routing",
    "suppress_gitbutler_prompt",
    "task_limit",
    "token_limit",
    "unstick",
    "updates",
    "url",
    "warn_after_secs",
    "webhook",
    "worktree_prefix",
];

fn reference(name: &str) -> &'static str {
    GUIDE_FILES.iter().find(|(path, _)| *path == name).expect("bundled reference").1
}

fn contains_token(text: &str, token: &str) -> bool {
    text.split(|c: char| !c.is_alphanumeric() && c != '_' && c != '-')
        .any(|word| word == token)
}

fn check_coverage(facts: &BTreeSet<String>, guide: &str, gaps: &[&str]) {
    let missing: BTreeSet<_> = facts.iter()
        .filter(|fact| !contains_token(guide, fact)).map(String::as_str).collect();
    let allowed: BTreeSet<_> = gaps.iter().copied().collect();
    assert_eq!(allowed.len(), gaps.len(), "duplicate documentation gap");
    assert_eq!(missing, allowed, "undocumented facts (or stale allowlist entries)");
}

#[test]
fn official_guide_covers_generated_facts() {
    assert!(UNDOCUMENTED_FLAGS.len() <= 64, "undocumented flags must only shrink");
    assert!(UNDOCUMENTED_CONFIG_KEYS.len() <= 36, "undocumented config keys must only shrink");
    let guide = GUIDE_FILES.iter().map(|(_, content)| *content).collect::<Vec<_>>().join("\n");
    let mut cli = crate::cli::Cli::command();
    cli.build();
    let mut flags = BTreeSet::new();
    let mut commands = vec![&cli];
    while let Some(command) = commands.pop() {
        flags.extend(command.get_arguments().filter_map(|arg| arg.get_long())
            .map(|long| format!("--{long}")));
        commands.extend(command.get_subcommands());
    }
    check_coverage(&flags, &guide, UNDOCUMENTED_FLAGS);

    let keys = RefCell::new(BTreeSet::new());
    let fields = Fields(&keys);
    crate::project::ProjectConfig::deserialize(fields).expect("project fields");
    crate::config::AidConfig::deserialize(fields).expect("global fields");
    // The budget accepts scalar shorthand through deserialize_with; inspect its table too.
    crate::project::ProjectBudget::deserialize(fields).expect("budget fields");
    check_coverage(&keys.borrow(), reference("references/configuration.md"), UNDOCUMENTED_CONFIG_KEYS);

    let statuses = crate::types::TaskStatus::ALL.iter().map(|status| status.as_str())
        .chain(crate::types::VerifyStatus::ALL.iter().map(|status| status.as_str()))
        .map(str::to_owned).collect();
    check_coverage(&statuses, reference("references/task-lifecycle.md"), &[]);

    let tools = crate::cmd::mcp_tools::tool_definitions().iter()
        .map(|tool| tool["name"].as_str().expect("MCP tool name").to_owned()).collect();
    check_coverage(&tools, reference("references/command-index.md"), &[]);
}

// Walk serde's actual field metadata, including nested options, arrays, and tables.
// Supplying every field prevents defaults/empty collections from hiding nested keys.
#[derive(Clone, Copy)]
struct Fields<'a>(&'a RefCell<BTreeSet<String>>);

type Error = serde::de::value::Error;

impl<'de> serde::Deserializer<'de> for Fields<'_> {
    type Error = Error;

    fn deserialize_any<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_i64(0)
    }

    fn deserialize_bool<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_bool(false)
    }

    fn deserialize_str<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_borrowed_str("")
    }

    fn deserialize_option<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_some(self)
    }

    fn deserialize_seq<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_tuple(1, visitor)
    }

    fn deserialize_tuple<V: Visitor<'de>>(self, len: usize, visitor: V) -> Result<V::Value, Error> {
        visitor.visit_seq(SeqDeserializer::new(std::iter::repeat_n(self, len)))
    }

    fn deserialize_struct<V: Visitor<'de>>(
        self, _name: &'static str, keys: &'static [&'static str], visitor: V,
    ) -> Result<V::Value, Error> {
        self.0.borrow_mut().extend(keys.iter().map(|key| (*key).to_owned()));
        visitor.visit_map(MapDeserializer::new(keys.iter().map(|key| (*key, self))))
    }

    serde::forward_to_deserialize_any! {
        i8 i16 i32 i64 u8 u16 u32 u64 f32 f64 char bytes byte_buf unit
        unit_struct newtype_struct tuple_struct map enum identifier ignored_any
    }

    fn deserialize_string<V: Visitor<'de>>(self, visitor: V) -> Result<V::Value, Error> {
        self.deserialize_str(visitor)
    }
}

impl<'de> IntoDeserializer<'de, Error> for Fields<'_> {
    type Deserializer = Self;

    fn into_deserializer(self) -> Self { self }
}
