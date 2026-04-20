//! `contextos` CLI — bridge between any editor / automation and the engine.
//!
//! Subcommands:
//!   optimize     — JSON in, optimised JSON out (unchanged from v0.1)
//!   build        — full-repo graph build
//!   update       — incremental graph update (list of changed files)
//!   impact       — print blast radius for a file
//!   skeleton     — signature-only view of a file
//!   watch        — long-running: update graph on every file save
//!   serve        — MCP JSON-RPC server on stdio
//!   stats        — graph stats from the current repo
//!   version      — print CLI version

mod install;
mod mcp;
mod watch;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use contextos_core_engine::{
    Engine, EngineConfig, OptimizationRequest, OptimizationResult,
};
use contextos_graph::Graph;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

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
        #[arg(short, long)]
        input: Option<PathBuf>,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long)]
        max_tokens: Option<usize>,
        #[arg(long)]
        pretty: bool,
    },
    /// Build (or refresh) the code graph for `--root` (defaults to cwd).
    Build {
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Incremental update — re-index only the listed files.
    Update {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Relative or absolute paths. If none are given, reads stdin
        /// (newline-separated) — useful with `git diff --name-only | contextos update`.
        files: Vec<PathBuf>,
    },
    /// Print the blast radius for one or more changed files.
    Impact {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, default_value = "2")]
        depth: u32,
        files: Vec<String>,
    },
    /// Print the signature-only skeleton for a file.
    Skeleton {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        path: String,
    },
    /// Watch the repo and auto-update the graph on save.
    Watch {
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Run as an MCP JSON-RPC server on stdio.
    Serve {
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Auto-configure Claude Code (writes .mcp.json + .claude/settings.local.json).
    Install {
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Remove the ContextOS entries from .mcp.json + settings.
    Uninstall {
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Graph stats (node/edge/file counts).
    Stats {
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Print the CLI version.
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
        Cmd::Build { root } => run_build(&root),
        Cmd::Update { root, files } => run_update(&root, files),
        Cmd::Impact { root, depth, files } => run_impact(&root, depth, files),
        Cmd::Skeleton { root, path } => run_skeleton(&root, &path),
        Cmd::Watch { root } => watch::run(&root),
        Cmd::Serve { root } => mcp::serve(&root),
        Cmd::Install { root } => run_install(&root),
        Cmd::Uninstall { root } => run_uninstall(&root),
        Cmd::Stats { root } => run_stats(&root),
    }
}

// ---- optimize -----------------------------------------------------------

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

// ---- graph commands ----------------------------------------------------

fn run_build(root: &Path) -> Result<()> {
    let graph = Graph::open(root)?;
    let report = graph.builder().build()?;
    println!(
        "build: scanned={} reparsed={} skipped={} nodes={} edges={}",
        report.files_scanned,
        report.files_reparsed,
        report.files_skipped,
        report.nodes_written,
        report.edges_written
    );
    Ok(())
}

fn run_update(root: &Path, files: Vec<PathBuf>) -> Result<()> {
    let files = if files.is_empty() {
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf.lines()
            .filter(|l| !l.trim().is_empty())
            .map(PathBuf::from)
            .collect()
    } else {
        files
    };
    let graph = Graph::open(root)?;
    let report = graph.builder().update(&files)?;
    println!(
        "update: scanned={} reparsed={} skipped={} nodes={} edges={}",
        report.files_scanned,
        report.files_reparsed,
        report.files_skipped,
        report.nodes_written,
        report.edges_written
    );
    Ok(())
}

fn run_impact(root: &Path, depth: u32, files: Vec<String>) -> Result<()> {
    let graph = Graph::open(root)?;
    let impact = graph.query().impact_radius(&files, depth)?;
    let paths: std::collections::BTreeSet<String> =
        impact.impacted.iter().map(|n| n.path.clone()).collect();
    for p in paths {
        println!("{p}");
    }
    eprintln!(
        "impact: seeds={} impacted_nodes={} unique_files={}",
        impact.seeds.len(),
        impact.impacted.len(),
        impact
            .impacted
            .iter()
            .map(|n| &n.path)
            .collect::<std::collections::HashSet<_>>()
            .len(),
    );
    Ok(())
}

fn run_skeleton(root: &Path, path: &str) -> Result<()> {
    let graph = Graph::open(root)?;
    let sk = graph.query().skeleton_for(path)?;
    if sk.trim().is_empty() {
        eprintln!("no skeleton for {path} — have you run `contextos build`?");
        std::process::exit(2);
    }
    print!("{sk}");
    Ok(())
}

fn run_install(root: &Path) -> Result<()> {
    let report = install::install(root)?;
    println!(
        "install: mcp_json={} settings={} already_configured={}",
        report.mcp_json_path.display(),
        report.settings_path.display(),
        report.already_configured
    );
    Ok(())
}

fn run_uninstall(root: &Path) -> Result<()> {
    install::uninstall(root)?;
    println!("uninstall: removed ContextOS entries from .mcp.json + .claude/settings.local.json");
    Ok(())
}

fn run_stats(root: &Path) -> Result<()> {
    let graph = Graph::open(root)?;
    let (nodes, edges, files) = graph.store.stats()?;
    println!("nodes={nodes} edges={edges} files={files}");
    Ok(())
}

// ---- io helpers --------------------------------------------------------

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
