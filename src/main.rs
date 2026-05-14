use std::io::IsTerminal;

use anyhow::Context;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "diffloom",
    about = "Save timeline, symbols, and MCP review bridge"
)]
struct Cli {
    #[command(subcommand)]
    command: Option<Cmd>,
    #[arg(
        long,
        help = "Workspace directory for the TUI (defaults to the current directory)"
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
            let root = cli
                .root
                .or_else(|| std::env::current_dir().ok())
                .context("set --root or run from a directory you can use as the workspace")?;
            diffloom::tui::run(root)?;
        }
    }
    Ok(())
}
