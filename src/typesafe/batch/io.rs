// JSONL preflight and append-only output for classify batches.
// Depends on std I/O and serde_json; parse errors never echo item state.

use super::{Item, invalid};
use crate::typesafe::classify::{ClassifyError, ClassifyState};
use serde_json::Value;
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::Path;

pub(crate) fn read_items(reader: impl BufRead) -> Result<Vec<Item>, ClassifyError> {
    let mut ids = HashSet::new();
    reader
        .lines()
        .enumerate()
        .map(|(index, line)| {
            let line = line.map_err(|_| invalid(format!("cannot read batch line {}", index + 1)))?;
            let value: Value = serde_json::from_str(&line)
                .map_err(|_| invalid(format!("batch line {} is not a JSON object", index + 1)))?;
            let id = value
                .get("id")
                .and_then(Value::as_str)
                .filter(|id| !id.trim().is_empty())
                .ok_or_else(|| invalid(format!("batch line {} needs a non-empty string id", index + 1)))?;
            if !ids.insert(id.to_string()) {
                return Err(invalid(format!("duplicate id at batch line {}", index + 1)));
            }
            let state = value
                .get("state")
                .ok_or_else(|| invalid(format!("batch line {} needs state", index + 1)))?;
            let state = match state {
                Value::String(text) => ClassifyState::Text(text.clone()),
                value => ClassifyState::Json(value.clone()),
            };
            Ok(Item {
                id: id.to_string(),
                state,
            })
        })
        .collect()
}

pub(crate) fn successful_ids(path: &Path) -> Result<HashSet<String>, ClassifyError> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(HashSet::new()),
        Err(_) => return Err(invalid("cannot read --out for resume")),
    };
    let mut ids = HashSet::new();
    for (index, line) in BufReader::new(file).lines().enumerate() {
        let bad = || {
            invalid(format!(
                "invalid resume output at line {}; repair it before resuming",
                index + 1
            ))
        };
        let value: Value = serde_json::from_str(&line.map_err(|_| bad())?).map_err(|_| bad())?;
        let id = value
            .get("id")
            .and_then(Value::as_str)
            .filter(|id| !id.trim().is_empty())
            .ok_or_else(bad)?;
        let ok = value.get("ok").and_then(Value::as_bool).ok_or_else(bad)?;
        if ok {
            ids.insert(id.to_string());
        }
    }
    Ok(ids)
}

pub(crate) fn open_output(input: &Path, output: &Path) -> Result<File, ClassifyError> {
    if same_file(input, output) {
        return Err(invalid("--batch and --out must be different files"));
    }
    let mut file = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(output)
        .map_err(|_| invalid("cannot open --out for append"))?;
    let len = file.metadata().map_err(|_| invalid("cannot inspect --out"))?.len();
    if len > 0 {
        file.seek(SeekFrom::End(-1)).map_err(|_| invalid("cannot seek --out"))?;
        let mut last = [0];
        file.read_exact(&mut last).map_err(|_| invalid("cannot read --out"))?;
        if last[0] != b'\n' {
            file.write_all(b"\n").map_err(|_| invalid("cannot append to --out"))?;
        }
    }
    Ok(file)
}

fn same_file(input: &Path, output: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        if let (Ok(left), Ok(right)) = (input.metadata(), output.metadata()) {
            return left.dev() == right.dev() && left.ino() == right.ino();
        }
    }
    match (input.canonicalize(), output.canonicalize()) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}
