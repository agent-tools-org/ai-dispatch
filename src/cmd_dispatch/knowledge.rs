// aid CLI knowledge and research dispatch handlers.
// Implements ask, query, memory, knowledge graph, findings, and broadcast.

use super::{resolve_finding_content, resolve_group};
use crate::cli::{FindingCommands, KgCommands, MemoryCommands};
use crate::cli_actions::{GroupAction, GroupFindingAction};
use crate::cmd;
use crate::store;
use anyhow::Result;
use std::sync::Arc;

pub(super) async fn ask(
    store: Arc<store::Store>,
    prompt: String,
    agent: Option<String>,
    model: Option<String>,
    files: Vec<String>,
    output: Option<String>,
) -> Result<()> {
    cmd::ask::run(store, prompt, agent, model, files, output).await
}

pub(super) fn query(
    store: Arc<store::Store>,
    prompt: String,
    auto: bool,
    model: Option<String>,
    group: Option<String>,
    finding: bool,
) -> Result<()> {
    let group = group.or_else(|| resolve_group(None));
    cmd::query::run(&store, &prompt, model.as_deref(), auto, group.as_deref(), finding)
}

pub(super) fn group(store: Arc<store::Store>, action: GroupAction) -> Result<()> {
    match action {
        GroupAction::Create { name, context, id } => {
            cmd::group::create(&store, &name, context.as_deref().unwrap_or(""), id.as_deref())
        }
        GroupAction::List => cmd::group::list(&store),
        GroupAction::Show { group_id } => cmd::group::show(&store, &group_id),
        GroupAction::Update { group_id, name, context } => {
            cmd::group::update(&store, &group_id, name.as_deref(), context.as_deref())
        }
        GroupAction::Delete { group_id, cascade } => cmd::group::delete(&store, &group_id, cascade),
        GroupAction::Cancel { group_id } => cmd::group::cancel(&store, &group_id),
        GroupAction::Summary { group_id } => cmd::summary_cli::run(&store, &group_id),
        GroupAction::Finding { action } => group_finding(store, action),
        GroupAction::Broadcast { group_id, message } => cmd::broadcast::run(&store, &group_id, &message),
    }
}

fn group_finding(store: Arc<store::Store>, action: GroupFindingAction) -> Result<()> {
    match action {
        GroupFindingAction::Add { group, content, stdin, file, task, severity, title, finding_file, lines, category, confidence } => {
            let content = resolve_finding_content(content, stdin, file)?;
            cmd::finding::add(&store, &group, &content, task.as_deref(), severity.as_deref(), title.as_deref(), finding_file.as_deref(), lines.as_deref(), category.as_deref(), confidence.as_deref())
        }
        GroupFindingAction::List { group, json, count, severity, verdict } => cmd::finding::list(&store, &group, json, count, severity.as_deref(), verdict.as_deref()),
        GroupFindingAction::Get { group, finding_id, json } => cmd::finding::get(&store, &group, finding_id, json),
        GroupFindingAction::Update { group, finding_id, verdict, score, note } => cmd::finding::update(&store, &group, finding_id, verdict.as_deref(), score.as_deref(), note.as_deref()),
    }
}

pub(super) fn memory(store: Arc<store::Store>, action: MemoryCommands) -> Result<()> {
    match action {
        MemoryCommands::Add { memory_type, content, tier, project } => {
            cmd::memory::add(&store, &memory_type, tier.as_deref(), &content, project.as_deref())
        }
        MemoryCommands::List { memory_type, all, project, stats } => {
            cmd::memory::list(&store, memory_type.as_deref(), project.as_deref(), all, stats)
        }
        MemoryCommands::Search { query, project } => {
            cmd::memory::search(&store, &query, project.as_deref())
        }
        MemoryCommands::Update { id, content } => cmd::memory::update(&store, &id, &content),
        MemoryCommands::Forget { id } => cmd::memory::forget(&store, &id),
        MemoryCommands::History { id } => cmd::memory::history(&store, &id),
    }
}

pub(super) fn kg(store: Arc<store::Store>, action: KgCommands) -> Result<()> {
    match action {
        KgCommands::Add { subject, predicate, object, valid_from, source } => {
            cmd::kg::add(&store, &subject, &predicate, &object, valid_from.as_deref(), source.as_deref())
        }
        KgCommands::Query { entity, as_of } => cmd::kg::query(&store, &entity, as_of.as_deref()),
        KgCommands::Invalidate { id } => cmd::kg::invalidate(&store, id),
        KgCommands::Timeline { entity } => cmd::kg::timeline(&store, &entity),
        KgCommands::Search { query } => cmd::kg::search(&store, &query),
        KgCommands::Stats => cmd::kg::stats(&store),
    }
}

pub(super) fn finding(store: Arc<store::Store>, action: FindingCommands) -> Result<()> {
    match action {
        FindingCommands::Add { group, content, stdin, file, task, severity, title, finding_file, lines, category, confidence } => {
            let content = resolve_finding_content(content, stdin, file)?;
            cmd::finding::add(&store, &group, &content, task.as_deref(), severity.as_deref(), title.as_deref(), finding_file.as_deref(), lines.as_deref(), category.as_deref(), confidence.as_deref())
        }
        FindingCommands::List { group, json, count, severity, verdict } => cmd::finding::list(&store, &group, json, count, severity.as_deref(), verdict.as_deref()),
        FindingCommands::Get { group, finding_id, json } => cmd::finding::get(&store, &group, finding_id, json),
        FindingCommands::Update { group, finding_id, verdict, score, note } => cmd::finding::update(&store, &group, finding_id, verdict.as_deref(), score.as_deref(), note.as_deref()),
    }
}

pub(super) fn broadcast(store: Arc<store::Store>, group: String, message: String) -> Result<()> {
    cmd::broadcast::run(&store, &group, &message)
}
