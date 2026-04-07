// CLI handler for `aid kg` — knowledge graph operations.
// Exports: add, query, invalidate, timeline, search, stats.
// Deps: crate::store::{KgStats, KgTriple, Store}.

use anyhow::{Result, bail};

use crate::store::{KgStats, KgTriple, Store};

pub(crate) fn add(
    store: &Store,
    subject: &str,
    predicate: &str,
    object: &str,
    valid_from: Option<&str>,
    source: Option<&str>,
) -> Result<()> {
    let id = store.add_kg_triple(subject, predicate, object, valid_from, source)?;
    println!("Triple {id} added");
    Ok(())
}

pub(crate) fn query(store: &Store, entity: &str, as_of: Option<&str>) -> Result<()> {
    let triples = store.query_kg_entity(entity, as_of)?;
    print_triple_table(&triples);
    Ok(())
}

pub(crate) fn invalidate(store: &Store, triple_id: i64) -> Result<()> {
    if !store.invalidate_kg_triple(triple_id)? {
        bail!("Triple {} not found or already invalidated", triple_id);
    }
    println!("Triple {triple_id} invalidated");
    Ok(())
}

pub(crate) fn timeline(store: &Store, entity: &str) -> Result<()> {
    let triples = store.kg_timeline(entity)?;
    if triples.is_empty() {
        println!("No triples found.");
        return Ok(());
    }
    for triple in triples {
        let point = triple.valid_from.as_ref().unwrap_or(&triple.created_at);
        println!(
            "{}  {}  {}  {}  [#{}]{}{}",
            point.format("%Y-%m-%d"),
            triple.subject,
            triple.predicate,
            triple.object,
            triple.id,
            format_valid_to_suffix(triple.valid_to.as_ref()),
            format_source_suffix(triple.source.as_deref()),
        );
    }
    Ok(())
}

pub(crate) fn search(store: &Store, query: &str) -> Result<()> {
    let triples = store.search_kg(query)?;
    print_triple_table(&triples);
    Ok(())
}

pub(crate) fn stats(store: &Store) -> Result<()> {
    let stats = store.kg_stats()?;
    print_stats(stats);
    Ok(())
}

fn print_triple_table(triples: &[KgTriple]) {
    if triples.is_empty() {
        println!("No triples found.");
        return;
    }
    println!(
        "{:<4} {:<15} {:<15} {:<15} {:<10} SOURCE",
        "ID", "SUBJECT", "PREDICATE", "OBJECT", "VALID FROM"
    );
    for triple in triples {
        println!(
            "{:<4} {:<15} {:<15} {:<15} {:<10} {}",
            triple.id,
            truncate(&triple.subject, 15),
            truncate(&triple.predicate, 15),
            truncate(&triple.object, 15),
            format_date(triple.valid_from.as_ref()),
            triple.source.as_deref().unwrap_or("-"),
        );
    }
}

fn print_stats(stats: KgStats) {
    println!("Triples: {}", stats.triple_count);
    println!("Active triples: {}", stats.active_triple_count);
    println!("Entities: {}", stats.entity_count);
    println!("Predicates: {}", stats.predicate_count);
}

fn format_date(value: Option<&chrono::DateTime<chrono::Local>>) -> String {
    value
        .map(|date| date.format("%Y-%m-%d").to_string())
        .unwrap_or_else(|| "-".to_string())
}

fn format_source_suffix(source: Option<&str>) -> String {
    match source {
        Some(value) => format!(" source={value}"),
        None => String::new(),
    }
}

fn format_valid_to_suffix(value: Option<&chrono::DateTime<chrono::Local>>) -> String {
    match value {
        Some(date) => format!(" until={}", date.format("%Y-%m-%d")),
        None => String::new(),
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.len() <= max {
        return value.to_string();
    }
    let safe = value.floor_char_boundary(max.saturating_sub(3));
    format!("{}...", &value[..safe])
}
