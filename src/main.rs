use anyhow::Context;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "diffloom",
    about = "Workspace timeline and symbols — graphical app by default; use `tui` or `mcp` for other modes"
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
    /// Terminal UI (requires an interactive tty)
    Tui,
    /// MCP server on stdio
    Mcp,
}

fn resolve_workspace(cli: &Cli, use_terminal_prompt: bool) -> anyhow::Result<std::path::PathBuf> {
    if let Some(p) = &cli.root {
        return diffloom::paths::normalize_path(p)
            .with_context(|| format!("workspace {}", p.display()));
    }
    if let Some(p) = diffloom::app_state::load_last_workspace()? {
        if let Ok(n) = diffloom::paths::normalize_path(&p) {
            return Ok(n);
        }
    }
    if use_terminal_prompt {
        let p = diffloom::app_state::prompt_workspace()?;
        return diffloom::paths::normalize_path(&p)
            .with_context(|| format!("workspace {}", p.display()));
    }
    let Some(p) = rfd::FileDialog::new()
        .set_title("Diffloom — choose workspace folder")
        .pick_folder()
    else {
        anyhow::bail!("No workspace folder selected.");
    };
    diffloom::paths::normalize_path(&p).with_context(|| format!("workspace {}", p.display()))
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
        Some(Cmd::Tui) => {
            use std::io::IsTerminal;
            if !std::io::stdout().is_terminal() {
                anyhow::bail!(
                    "TUI needs an interactive terminal. Use the default GUI, or run `diffloom mcp` for stdio MCP mode."
                );
            }
            let root = resolve_workspace(&cli, true)?;
            diffloom::app_state::save_last_workspace(&root)?;
            diffloom::tui::run(root)?;
        }
        None => {
            let root = resolve_workspace(&cli, false)?;
            diffloom::app_state::save_last_workspace(&root)?;
            diffloom::gui::run(root)?;
        }
    }
    Ok(())
}
