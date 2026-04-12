// Project-scoped knowledge loading helpers.
// Exports: project_knowledge_dir() and read_project_knowledge().
// Deps: std::fs/path and crate::team knowledge parsing.

use std::fs;
use std::path::{Path, PathBuf};

use crate::team::{self, KnowledgeEntry};

pub fn project_knowledge_dir(git_root: &Path) -> PathBuf {
    git_root.join(".aid").join("knowledge")
}

pub fn read_project_knowledge(git_root: &Path) -> Vec<KnowledgeEntry> {
    let knowledge_dir = project_knowledge_dir(git_root);
    let index_path = knowledge_dir.join("KNOWLEDGE.md");
    let raw = match fs::read_to_string(&index_path) {
        Ok(body) => body,
        Err(_) => return Vec::new(),
    };
    raw.lines()
        .filter_map(|line| team::parse_knowledge_line(line, &knowledge_dir))
        .collect()
}
