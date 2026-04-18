//! `contextos` CLI — thin wrapper around the core engine.
//!
//! Modes:
//!   * `optimize`   read an OptimizationRequest as JSON and write the
//!                  OptimizationResult back as JSON. Default: stdin/stdout,
//!                  so the VS Code extension can pipe in/out.
//!   * `version`    print the CLI version. The extension calls this during
//!                  its activation handshake.

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use contextos_core_engine::{
    Engine, EngineConfig, OptimizationRequest, OptimizationResult,
};
use std::io::{Read, Write};
use std::path::PathBuf;

#[derive(Parser)]
#[command(
    name = "contextos",
    version,
    about = "ContextOS: local token-reduction engine for AI code editors"
)]
struct Cli {
    #[command(subcommand)]
    command: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// Run the optimization pipeline on JSON input.
    Optimize {
        /// Path to request JSON. Defaults to stdin.
        #[arg(short, long)]
        input: Option<PathBuf>,
        /// Path to write result JSON. Defaults to stdout.
        #[arg(short, long)]
        output: Option<PathBuf>,
        /// Override the token budget.
        #[arg(long)]
        max_tokens: Option<usize>,
        /// Emit pretty-printed JSON.
        #[arg(long)]
        pretty: bool,
    },
    /// Print the CLI version. Used by the extension for the handshake.
    Version,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Cmd::Version => {
            println!("{}", env!("CARGO_PKG_VERSION"));
            Ok(())
        }
        Cmd::Optimize {
            input,
            output,
            max_tokens,
            pretty,
        } => run_optimize(input, output, max_tokens, pretty),
    }
}

fn run_optimize(
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    max_tokens: Option<usize>,
    pretty: bool,
) -> Result<()> {
    let raw = read_input(input)?;
    let request: OptimizationRequest = serde_json::from_str(&raw)
        .context("failed to parse OptimizationRequest JSON")?;

    let mut cfg = EngineConfig::default();
    if let Some(t) = max_tokens {
        cfg.max_tokens = t;
    }

    let engine = Engine::new(cfg);
    let result: OptimizationResult = engine.optimize(request);

    let serialized = if pretty {
        serde_json::to_string_pretty(&result)?
    } else {
        serde_json::to_string(&result)?
    };
    write_output(output, &serialized)
}

fn read_input(path: Option<PathBuf>) -> Result<String> {
    match path {
        Some(p) => std::fs::read_to_string(&p)
            .with_context(|| format!("reading input from {}", p.display())),
        None => {
            let mut s = String::new();
            std::io::stdin()
                .read_to_string(&mut s)
                .context("reading input from stdin")?;
            Ok(s)
        }
    }
}

fn write_output(path: Option<PathBuf>, data: &str) -> Result<()> {
    match path {
        Some(p) => std::fs::write(&p, data)
            .with_context(|| format!("writing output to {}", p.display())),
        None => {
            let stdout = std::io::stdout();
            let mut handle = stdout.lock();
            handle.write_all(data.as_bytes())?;
            handle.write_all(b"\n")?;
            Ok(())
        }
    }
}
