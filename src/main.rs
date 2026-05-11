use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand, error::ErrorKind as ClapErrorKind};
use serde::Serialize;

#[derive(Debug, Parser)]
#[command(name = "codex-wt")]
#[command(about = "Create Codex-style detached Git worktrees")]
#[command(color = clap::ColorChoice::Never)]
struct Cli {
    #[arg(long, global = true, help = "Emit stable JSON")]
    json: bool,

    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    #[command(about = "Check local setup")]
    Doctor,
    #[command(about = "Create a detached worktree under $CODEX_HOME/worktrees")]
    Create {
        #[arg(long, help = "Source Git repository path")]
        repo: PathBuf,
        #[arg(long, help = "Base ref, branch, tag, or commit")]
        base: String,
        #[arg(long, help = "Human-readable worktree id prefix")]
        slug: Option<String>,
        #[arg(long, help = "Apply tracked dirty changes from source repo")]
        include_dirty: bool,
        #[arg(long, help = "Copy untracked non-ignored files from source repo")]
        include_untracked: bool,
    },
    #[command(about = "List Git worktrees for a repository")]
    List {
        #[arg(long, help = "Repository path")]
        repo: PathBuf,
    },
    #[command(about = "Inspect one worktree")]
    Inspect {
        #[arg(long, help = "Worktree path")]
        path: PathBuf,
    },
}

fn main() -> ExitCode {
    let json_requested = std::env::args_os().any(|arg| arg == "--json");
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            if matches!(
                error.kind(),
                ClapErrorKind::DisplayHelp | ClapErrorKind::DisplayVersion
            ) {
                error.exit();
            }
            if json_requested {
                print_json(&ErrorOutput::from_message(error.to_string()));
                return ExitCode::FAILURE;
            }
            error.exit();
        }
    };

    match run(&cli) {
        Ok(output) => {
            if cli.json {
                print_json(&output);
            } else {
                println!("{}", output.human);
            }
            ExitCode::SUCCESS
        }
        Err(error) => {
            if cli.json {
                print_json(&ErrorOutput::from_error(&error));
            } else {
                eprintln!("{error:#}");
            }
            ExitCode::FAILURE
        }
    }
}

fn run(cli: &Cli) -> anyhow::Result<CommandOutput> {
    match &cli.command {
        Command::Doctor => {
            let report = codex_wt::doctor();
            Ok(CommandOutput::new(
                format!(
                    "git: {}\nCODEX_HOME: {}\nworktrees: {}",
                    report.git_version.as_deref().unwrap_or("missing"),
                    report.codex_home.display(),
                    report.worktrees_dir.display()
                ),
                report,
            ))
        }
        Command::Create {
            repo,
            base,
            slug,
            include_dirty,
            include_untracked,
        } => {
            let result = codex_wt::create(codex_wt::CreateOptions {
                repo: repo.clone(),
                base: base.clone(),
                slug: slug.clone(),
                include_dirty: *include_dirty,
                include_untracked: *include_untracked,
            })?;
            Ok(CommandOutput::new(
                result.path.display().to_string(),
                result,
            ))
        }
        Command::List { repo } => {
            let result = codex_wt::list(repo)?;
            Ok(CommandOutput::new(
                result
                    .worktrees
                    .iter()
                    .map(|worktree| {
                        let state = worktree.branch.as_deref().unwrap_or(if worktree.detached {
                            "detached"
                        } else {
                            "unknown"
                        });
                        format!("{}\t{}", worktree.path.display(), state)
                    })
                    .collect::<Vec<_>>()
                    .join("\n"),
                result,
            ))
        }
        Command::Inspect { path } => {
            let result = codex_wt::inspect(path)?;
            Ok(CommandOutput::new(
                format!(
                    "{}\n{}",
                    result.path.display(),
                    result.branch.as_deref().unwrap_or(if result.detached {
                        "detached"
                    } else {
                        "unknown"
                    })
                ),
                result,
            ))
        }
    }
}

#[derive(Debug, Serialize)]
struct CommandOutput {
    #[serde(skip_serializing)]
    human: String,
    #[serde(flatten)]
    value: serde_json::Value,
}

impl CommandOutput {
    fn new<T: Serialize>(human: String, value: T) -> Self {
        Self {
            human,
            value: serde_json::to_value(value).expect("serializable command output"),
        }
    }
}

#[derive(Debug, Serialize)]
struct ErrorOutput {
    ok: bool,
    error: ErrorBody,
}

#[derive(Debug, Serialize)]
struct ErrorBody {
    message: String,
}

impl ErrorOutput {
    fn from_error(error: &anyhow::Error) -> Self {
        Self::from_message(format!("{error:#}"))
    }

    fn from_message(message: String) -> Self {
        Self {
            ok: false,
            error: ErrorBody { message },
        }
    }
}

fn print_json<T: Serialize>(value: &T) {
    println!(
        "{}",
        serde_json::to_string_pretty(value).expect("serializable JSON output")
    );
}
