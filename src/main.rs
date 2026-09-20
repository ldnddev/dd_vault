use std::path::PathBuf;
use std::process::ExitCode;

use anyhow::Result;
use clap::{Parser, Subcommand};
use dd_vault_core::{init, open, reindex, Paths, Registry, Vault};

#[derive(Debug, Parser)]
#[command(
    name = "dd_vault",
    version = env!("CARGO_PKG_VERSION"),
    about = "Keyboard-first vim-flavored markdown knowledge vault TUI"
)]
struct Cli {
    /// Rebuild the SQLite + FTS5 derived index and exit (no TUI).
    #[arg(long)]
    reindex: bool,

    /// Vault path for --reindex (default: last opened vault)
    #[arg(requires = "reindex")]
    path: Option<PathBuf>,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Create a vault (folder, .dd_vault-<name>/, .gitignore, notes/, assets/)
    Init {
        /// Directory to create or reuse (default: current directory)
        path: Option<PathBuf>,
    },
    /// Register a vault and open the TUI
    Open { path: PathBuf },
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(err) => {
            eprintln!("{err:#}");
            ExitCode::from(1)
        }
    }
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    if cli.reindex {
        if cli.command.is_some() {
            anyhow::bail!("--reindex cannot be combined with a subcommand");
        }
        return cmd_reindex(cli.path);
    }
    match cli.command {
        Some(Command::Init { path }) => cmd_init(path),
        Some(Command::Open { path }) => {
            let vault = open(&path)?;
            register(&vault)?;
            dd_tui::run(Some(vault))?;
            Ok(())
        }
        None => dd_tui::run(None),
    }
}

fn cmd_init(path: Option<PathBuf>) -> Result<()> {
    let path = path.unwrap_or_else(|| PathBuf::from("."));
    let vault = init(&path)?;
    register(&vault)?;
    println!(
        "Initialized vault '{}' at {}\nmetadata: {}",
        vault.name,
        vault.root.display(),
        vault.meta_dir.display()
    );
    Ok(())
}

fn cmd_reindex(path: Option<PathBuf>) -> Result<()> {
    let path = match path {
        Some(p) => p,
        None => Registry::load(&Paths::from_env()?)?
            .last_path()
            .ok_or(dd_vault_core::Error::NoVaultToReindex)?,
    };
    let vault = open(&path)?;
    let report = reindex(&vault)?;
    println!("vault: {} ({})", vault.name, vault.root.display());
    println!("markdown files: {}", report.markdown_files);
    println!("other files: {}", report.other_files);
    println!("notes indexed: {}", report.notes_indexed);
    Ok(())
}

fn register(vault: &Vault) -> Result<()> {
    let paths = Paths::from_env()?;
    let mut registry = Registry::load(&paths)?;
    registry.register(vault);
    registry.save(&paths)?;
    Ok(())
}
