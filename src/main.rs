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
    #[arg(long, help = "Workspace root (required for interactive UI)")]
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
            let root = cli
                .root
                .context("missing --root (workspace directory for the TUI watcher)")?;
            diffloom::tui::run(root)?;
        }
    }
    Ok(())
}
