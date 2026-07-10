use clap::{Parser, Subcommand, ValueEnum};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

mod config;
mod engine;
mod output;

#[derive(Debug, Parser)]
#[command(
    name = "prepo",
    version,
    about = "Preflight checks for development repositories."
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a placeholder prepo.toml in the current directory.
    Init,
    /// Validate a prepo.toml without executing checks.
    Validate {
        #[arg(long)]
        config: Option<PathBuf>,
    },
    /// Run declared preflight checks.
    Check {
        #[arg(long)]
        config: Option<PathBuf>,
        #[arg(long, value_enum, default_value_t = FormatArg::Human)]
        format: FormatArg,
        #[arg(long)]
        sequential: bool,
        #[arg(long)]
        no_color: bool,
        #[arg(long)]
        allow_command: bool,
        #[arg(long)]
        allow_network: bool,
        #[arg(long)]
        allow_env: bool,
        #[arg(long, conflicts_with = "ids")]
        group: Option<String>,
        #[arg(value_name = "ID")]
        ids: Vec<String>,
    },
}

#[derive(Clone, Copy, Debug, ValueEnum, PartialEq)]
enum FormatArg {
    Human,
    Json,
    Markdown,
}

fn main() {
    let exit_code = match Cli::parse().command {
        Command::Init => init(),
        Command::Validate { config } => validate(config),
        Command::Check {
            config,
            format,
            sequential,
            no_color,
            allow_command,
            allow_network,
            allow_env,
            group,
            ids,
        } => check(
            config,
            format,
            sequential,
            no_color,
            allow_command,
            allow_network,
            allow_env,
            group,
            ids,
        ),
    };

    std::process::exit(exit_code);
}

#[allow(clippy::too_many_arguments)]
fn check(
    config_path: Option<PathBuf>,
    format: FormatArg,
    _sequential: bool,
    no_color: bool,
    allow_command: bool,
    allow_network: bool,
    allow_env: bool,
    group: Option<String>,
    ids: Vec<String>,
) -> i32 {
    if no_color && format != FormatArg::Human {
        eprintln!("error: --no-color is only valid with --format human");
        return 2;
    }
    let mut ids_seen = std::collections::BTreeSet::new();
    if ids.iter().any(|id| !ids_seen.insert(id)) {
        eprintln!("error: an ID may be selected only once");
        return 2;
    }
    let config = match config::load_and_validate(config_path.as_deref()) {
        Ok(config) => config,
        Err(error) => {
            eprintln!("error: {error}");
            return 2;
        }
    };
    if config::Platform::current().is_none() {
        eprintln!("error: unsupported operating system");
        return 3;
    }
    if allow_command {
        eprintln!(
            "warning: --allow-command permits configured commands with inherited environment, filesystem, and network access"
        );
    }
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(_) => {
            eprintln!("error: prepo runtime initialization failed");
            return 3;
        }
    };
    let run = match runtime.block_on(engine::run(
        &config,
        &ids,
        group.as_deref(),
        engine::Permissions {
            command: allow_command,
            network: allow_network,
            env: allow_env,
        },
    )) {
        Ok(run) => run,
        Err(engine::SelectionError::UnknownId(id)) => {
            eprintln!("error: unknown check ID: {id}");
            return 4;
        }
        Err(engine::SelectionError::EmptyGroup(group)) => {
            eprintln!("error: group has no checks: {group}");
            return 4;
        }
    };
    let output_format = match format {
        FormatArg::Human => output::Format::Human,
        FormatArg::Json => output::Format::Json,
        FormatArg::Markdown => output::Format::Markdown,
    };
    let color = !no_color
        && std::io::IsTerminal::is_terminal(&std::io::stdout())
        && std::env::var_os("NO_COLOR").is_none();
    match output::write_run(&run, output_format, color) {
        Ok(()) => engine::exit_code(&run),
        Err(error) if error.kind() == std::io::ErrorKind::BrokenPipe => 0,
        Err(_) => {
            eprintln!("error: cannot write output");
            3
        }
    }
}

fn validate(path: Option<PathBuf>) -> i32 {
    match config::load_and_validate(path.as_deref()) {
        Ok(validated) => {
            let _ = validated.path;
            println!("Configuration is valid.");
            0
        }
        Err(error) => {
            eprintln!("error: {error}");
            2
        }
    }
}

fn init() -> i32 {
    let directory = match std::env::current_dir() {
        Ok(directory) => directory,
        Err(error) => {
            eprintln!("error: cannot determine the current directory: {error}");
            return 2;
        }
    };
    let project_name = match directory.file_name().and_then(|name| name.to_str()) {
        Some(name) if !name.trim().is_empty() => name,
        _ => {
            eprintln!("error: cannot derive a project name from the current directory");
            return 2;
        }
    };
    let path = directory.join("prepo.toml");
    let template = format!(
        "version = 1\n\n[project]\nname = {project_name:?}\n\n# Replace this check with a real project prerequisite.\n[[checks]]\nid = \"replace-me\"\ntype = \"path\"\nname = \"Replace this example check\"\npath = \"REPLACE-ME\"\nexpected = \"exists\"\nkind = \"any\"\nhelp = \"Replace this placeholder with a project prerequisite.\"\n"
    );

    match create_new(&path, template.as_bytes()) {
        Ok(()) => {
            println!("Created prepo.toml");
            0
        }
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
            eprintln!("error: prepo.toml already exists");
            2
        }
        Err(error) => {
            eprintln!("error: cannot create prepo.toml: {error}");
            2
        }
    }
}

fn create_new(path: &PathBuf, content: &[u8]) -> std::io::Result<()> {
    let mut file = OpenOptions::new().write(true).create_new(true).open(path)?;
    file.write_all(content)
}
