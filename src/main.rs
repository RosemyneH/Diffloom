use std::io::IsTerminal;

use anyhow::Context;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "diffloom",
    about = "Standalone workspace timeline and symbol watcher (per-project data under .diffloom/)"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
    #[arg(
        long,
        help = "Project directory to watch (saved for next launch if omitted)"
    )]
    root: Option<std::path::PathBuf>,
}

#[derive(Subcommand)]
enum Cmd {
    Mcp,
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();
    let cli = Cli::parse();
    match cli.command {
        Some(Cmd::Mcp) => {
            let rt = tokio::runtime::Runtime::new()?;
            rt.block_on(diffloom::mcp_server::run_stdio())?;
        }
        None => {
            if !std::io::stdout().is_terminal() {
                anyhow::bail!(
                    "TUI needs an interactive terminal. Open a real terminal, or run `diffloom mcp` for stdio MCP mode."
                );
            }
            let root = if let Some(p) = cli.root {
                p
            } else if let Some(p) = diffloom::app_state::load_last_workspace()? {
                p
            } else {
                diffloom::app_state::prompt_workspace()?
            };
            let root = diffloom::paths::normalize_path(&root)
                .with_context(|| format!("workspace {}", root.display()))?;
            diffloom::app_state::save_last_workspace(&root)?;
            diffloom::tui::run(root)?;
        }
    }
    Ok(())
}
