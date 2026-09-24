// CLI arguments for `aid classify`: typed questions about a text or file, answered by TypeSafe Jev.
// Exports: ClassifyArgs. Question flags are kept in argv order so each --choice pairs with the next
// --options and each --score with the next --levels. Deps: clap, typesafe::question.

use clap::{ArgMatches, Args, Command, FromArgMatches};

use crate::typesafe::question::{FlagEvent, FlagKind};

#[derive(Args)]
pub struct ClassifyFlags {
    /// State file, or - for stdin (default: stdin)
    #[arg(long)]
    pub state: Option<String>,
    /// Parse the state as JSON (object or array) instead of text
    #[arg(long)]
    pub state_json: bool,
    /// Yes/no question: ID=QUESTION (repeatable)
    #[arg(long, value_name = "ID=QUESTION")]
    pub noul: Vec<String>,
    /// Choice question: ID=QUESTION, followed by its --options (repeatable)
    #[arg(long, value_name = "ID=QUESTION")]
    pub choice: Vec<String>,
    /// Options for the preceding --choice: A,B,C (2-255)
    #[arg(long, value_name = "A,B,C")]
    pub options: Vec<String>,
    /// Score question: ID=QUESTION, followed by its --levels (repeatable)
    #[arg(long, value_name = "ID=QUESTION")]
    pub score: Vec<String>,
    /// Levels for the preceding --score, lowest first: L0,L1,... (2-10)
    #[arg(long, value_name = "L0,L1,...")]
    pub levels: Vec<String>,
    /// Native TypeSafe questions map (JSON file); merged with the flags
    #[arg(long, value_name = "FILE")]
    pub questions: Option<String>,
    /// Model to ask
    #[arg(long, default_value = crate::typesafe::classify::DEFAULT_MODEL)]
    pub model: String,
    /// Request timeout in seconds
    #[arg(long, default_value_t = crate::typesafe::classify::DEFAULT_TIMEOUT_SECS)]
    pub timeout: u64,
    /// Send state even when it contains a PEM block or a token-like prefix
    #[arg(long)]
    pub allow_secret_like: bool,
}

pub struct ClassifyArgs {
    pub flags: ClassifyFlags,
    /// --noul/--choice/--options/--score/--levels values in argv order.
    pub events: Vec<FlagEvent>,
}

impl FromArgMatches for ClassifyArgs {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
        let flags = ClassifyFlags::from_arg_matches(matches)?;
        let mut indexed = Vec::new();
        for (id, kind) in [
            ("noul", FlagKind::Noul),
            ("choice", FlagKind::Choice),
            ("options", FlagKind::Options),
            ("score", FlagKind::Score),
            ("levels", FlagKind::Levels),
        ] {
            if let (Some(values), Some(indices)) = (matches.get_many::<String>(id), matches.indices_of(id)) {
                indexed.extend(indices.zip(values).map(|(index, value)| (index, kind, value.clone())));
            }
        }
        indexed.sort_by_key(|(index, _, _)| *index);
        let events = indexed.into_iter().map(|(_, kind, value)| FlagEvent { kind, value }).collect();
        Ok(Self { flags, events })
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), clap::Error> {
        *self = Self::from_arg_matches(matches)?;
        Ok(())
    }
}

impl Args for ClassifyArgs {
    fn augment_args(cmd: Command) -> Command {
        ClassifyFlags::augment_args(cmd)
    }

    fn augment_args_for_update(cmd: Command) -> Command {
        ClassifyFlags::augment_args_for_update(cmd)
    }
}

#[cfg(test)]
mod tests {
    use clap::Parser;

    use crate::cli::{Cli, Commands};
    use crate::typesafe::question::{FlagKind, questions_from_flags};

    fn parse(args: &[&str]) -> super::ClassifyArgs {
        let argv = ["aid", "classify"].iter().chain(args).copied();
        let Some(Commands::Classify(parsed)) = Cli::try_parse_from(argv).expect("parse").command else {
            panic!("expected classify");
        };
        parsed
    }

    #[test]
    fn defaults_read_stdin_with_pinned_model() {
        let args = parse(&["--noul", "a=A?"]);
        assert_eq!(args.flags.state, None);
        assert_eq!(args.flags.model, "jev-1.13.0");
        assert_eq!(args.flags.timeout, 20);
        assert!(!args.flags.state_json && !args.flags.allow_secret_like);
    }

    #[test]
    fn events_keep_argv_order_across_flag_kinds() {
        let args = parse(&[
            "--choice", "v=Verdict?", "--options", "SHIP,FIX", "--noul", "t=Tests?",
            "--choice", "l=Lang?", "--options", "rust,go", "--score", "r=Risk?", "--levels", "lo,hi",
        ]);
        let kinds: Vec<FlagKind> = args.events.iter().map(|event| event.kind).collect();
        assert_eq!(kinds, [
            FlagKind::Choice, FlagKind::Options, FlagKind::Noul, FlagKind::Choice,
            FlagKind::Options, FlagKind::Score, FlagKind::Levels,
        ]);
        let built = questions_from_flags(&args.events).expect("paired");
        let lang = built.iter().find(|(id, _)| id == "l").expect("lang");
        assert_eq!(lang.1["criteria"], serde_json::json!({ "rust": null, "go": null }));
    }

    #[test]
    fn grouped_choices_do_not_pair_with_later_options() {
        let args = parse(&["--choice", "a=A?", "--choice", "b=B?", "--options", "x,y", "--options", "p,q"]);
        assert!(questions_from_flags(&args.events).is_err());
    }
}
