// CLI batch setup: validate the entire input, resolve the key once, append results.
// Exports: run; depends on the shared classify template and TypeSafe batch engine.

use crate::cli::command_args_classify::ClassifyArgs;
use crate::typesafe::batch::{self, Summary};
use crate::typesafe::classify::{BatchClassifier, ClassifyError, ErrorKind};
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

pub(super) fn run(args: &ClassifyArgs) -> i32 {
    match execute(args) {
        Ok(summary) => {
            eprint!("{summary}");
            summary.exit_code()
        }
        Err(error) => {
            eprintln!("aid classify: {error}");
            error.exit_code()
        }
    }
}

fn execute(args: &ClassifyArgs) -> Result<Summary, ClassifyError> {
    let invalid = |message| ClassifyError::new(ErrorKind::Invalid, message);
    let template = super::classify::request_template(args)?;
    // Validate model/timeout even when the input is empty or every id is resumed.
    let declared = crate::typesafe::classify::prepare(&template)?;
    let input = Path::new(
        args.flags
            .batch
            .as_deref()
            .ok_or_else(|| invalid("--batch is required"))?,
    );
    let output = Path::new(args.flags.out.as_deref().ok_or_else(|| invalid("--out is required"))?);
    let file = File::open(input).map_err(|_| invalid("cannot read --batch"))?;
    let mut items = batch::read_items(BufReader::new(file))?;
    let total = items.len();
    if args.flags.resume {
        let completed = batch::successful_ids(output)?;
        items.retain(|item| !completed.contains(&item.id));
    }
    let skipped = total - items.len();
    let mut output = batch::open_output(input, output)?;
    if items.is_empty() {
        return Ok(Summary::new(total, skipped, &declared));
    }
    let classifier = BatchClassifier::new()?;
    batch::run(
        items,
        &template,
        usize::from(args.flags.jobs.unwrap_or(4)),
        skipped,
        &mut output,
        &|request| classifier.classify(request),
    )
}
