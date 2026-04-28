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
    ///
    /// Pass `--graph-root` to enable graph-aware priors (Personalized
    /// PageRank seeded by query terms, plus betweenness centrality fused
    /// in via RRF) and Louvain community-balanced budget allocation.
    /// Without `--graph-root` the pipeline output is byte-identical to
    /// the graph-free engine path.
    Optimize {
        #[arg(short, long)]
        input: Option<PathBuf>,
        #[arg(short, long)]
        output: Option<PathBuf>,
        #[arg(long)]
        max_tokens: Option<usize>,
        #[arg(long)]
        pretty: bool,
        /// Optional path to a repo with a `.contextos/graph.db` index.
        /// When set, the pipeline computes betweenness + personalized
        /// PageRank priors from the graph and assigns Louvain communities
        /// to chunks based on their `path`. Off by default.
        #[arg(long)]
        graph_root: Option<PathBuf>,
        /// Enable RM3 query expansion. Off by default.
        #[arg(long, default_value_t = false)]
        rm3: bool,
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
    ///
    /// `--strategy bfs` (default) preserves the original BFS reverse-edge
    /// traversal output. `rwr` and `steiner` are graph-aware
    /// alternatives that may produce different — but lossless — node
    /// sets. Default behavior is unchanged from earlier releases.
    Impact {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, default_value = "2")]
        depth: u32,
        #[arg(long, default_value = "bfs", value_parser = ["bfs", "rwr", "steiner"])]
        strategy: String,
        /// When set, the result is forward-reachable-pruned from the
        /// changed files: anything not actually reachable through call/
        /// import edges is dropped.
        #[arg(long, default_value_t = false)]
        prune_unreachable: bool,
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
    /// One-shot setup: build the graph and wire Claude Code in a single
    /// command. Equivalent to running `build` then `install`.
    Init {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        /// Skip the graph build (only write `.mcp.json`). Useful if the
        /// graph has already been built or you want zero-cost first
        /// run — Claude Code will index lazily on first MCP call.
        #[arg(long, default_value_t = false)]
        skip_build: bool,
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
    /// Show cumulative token savings across all `optimize` calls,
    /// CLI + MCP combined. Reads `~/.contextos/usage.jsonl`. Output is
    /// a colored ASCII dashboard.
    Savings {
        /// Top N rows to show in the by-query table.
        #[arg(long, default_value = "10")]
        top: usize,
        /// Filter to a specific project root (default: all projects).
        #[arg(long)]
        project: Option<PathBuf>,
        /// Skip the colored output (useful when redirecting to a file).
        #[arg(long, default_value_t = false)]
        no_color: bool,
    },
    /// Print the top-k nodes by random-walk-with-restart score from the
    /// given seed nodes (referenced by `path`).
    Rwr {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, default_value = "20")]
        top_k: usize,
        seeds: Vec<String>,
    },
    /// Compute Louvain communities and print one community per line as
    /// `<community_id>\t<path>`.
    Communities {
        #[arg(long, default_value = ".")]
        root: PathBuf,
    },
    /// Print betweenness centrality (sampled) for every node, sorted
    /// descending — useful for spotting bridge nodes.
    Bridges {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        #[arg(long, default_value = "50")]
        top_k: usize,
    },
    /// Print the approximate Steiner subgraph nodes connecting a set of
    /// terminal symbols (looked up by `name`).
    Steiner {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        names: Vec<String>,
    },
    /// Print the forward-reachable closure of the given root files.
    Reachable {
        #[arg(long, default_value = ".")]
        root: PathBuf,
        files: Vec<String>,
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
            graph_root,
            rm3,
        } => run_optimize(input, output, max_tokens, pretty, graph_root, rm3),
        Cmd::Build { root } => run_build(&root),
        Cmd::Update { root, files } => run_update(&root, files),
        Cmd::Impact {
            root,
            depth,
            strategy,
            prune_unreachable,
            files,
        } => run_impact(&root, depth, &strategy, prune_unreachable, files),
        Cmd::Skeleton { root, path } => run_skeleton(&root, &path),
        Cmd::Watch { root } => watch::run(&root),
        Cmd::Serve { root } => mcp::serve(&root),
        Cmd::Init { root, skip_build } => run_init(&root, skip_build),
        Cmd::Install { root } => run_install(&root),
        Cmd::Uninstall { root } => run_uninstall(&root),
        Cmd::Stats { root } => run_stats(&root),
        Cmd::Savings {
            top,
            project,
            no_color,
        } => run_savings(top, project.as_deref(), no_color),
        Cmd::Rwr { root, top_k, seeds } => run_rwr(&root, top_k, seeds),
        Cmd::Communities { root } => run_communities(&root),
        Cmd::Bridges { root, top_k } => run_bridges(&root, top_k),
        Cmd::Steiner { root, names } => run_steiner(&root, names),
        Cmd::Reachable { root, files } => run_reachable(&root, files),
    }
}

// ---- optimize -----------------------------------------------------------

fn run_optimize(
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    max_tokens: Option<usize>,
    pretty: bool,
    graph_root: Option<PathBuf>,
    rm3: bool,
) -> Result<()> {
    let raw = read_input(input)?;
    let mut request: OptimizationRequest = serde_json::from_str(&raw)
        .context("failed to parse OptimizationRequest JSON")?;
    let chunks_in = request.chunks.len();
    let query = request.query.clone();
    let mut cfg = EngineConfig::default();
    if let Some(t) = max_tokens {
        cfg.max_tokens = t;
    }
    cfg.enable_rm3 = rm3;

    let result: OptimizationResult = match graph_root.as_deref() {
        // No graph: original graph-free path. Byte-identical to earlier releases.
        None => Engine::new(cfg).optimize(request),
        Some(root) => {
            let priors = compute_graph_priors(root, &request)?;
            // Tag chunks with their dominant Louvain community (if any
            // graph node lives at the chunk's path) and turn on the
            // community-aware budget objective.
            tag_chunk_communities(root, &mut request)?;
            cfg.enable_louvain_budget = true;
            Engine::new(cfg).optimize_with_priors(request, Some(&priors))
        }
    };

    // Telemetry: append a UsageRecord to ~/.contextos/usage.jsonl so
    // `contextos savings` can show cumulative reductions later. No-op
    // when CONTEXTOS_NO_USAGE is set.
    contextos_utils::record_usage(contextos_utils::UsageRecord {
        ts: 0, // filled in by record()
        in_tokens: result.original_tokens,
        out_tokens: result.final_tokens,
        saved_tokens: result.tokens_saved,
        elapsed_ms: result.elapsed_ms,
        query,
        chunks_in,
        chunks_out: result.chunks.len(),
        source: "cli".into(),
        project: std::env::current_dir()
            .ok()
            .map(|p| p.to_string_lossy().into_owned()),
    });

    let serialized = if pretty {
        serde_json::to_string_pretty(&result)?
    } else {
        serde_json::to_string(&result)?
    };
    write_output(output, &serialized)
}

/// Build a `Priors` map for the engine from graph-derived signals:
/// personalized PageRank (seeded by query terms found in the graph) +
/// betweenness centrality, blended via simple addition. Both signals
/// live on tiny scales (1e-4 to 1e-1), so the RRF inside ranking will
/// rank-fuse them with BM25 anyway — we just need to surface a
/// per-chunk score.
fn compute_graph_priors(
    root: &Path,
    request: &OptimizationRequest,
) -> Result<contextos_core_engine::ranking::Priors> {
    use contextos_core_engine::ranking::Priors;
    let graph = Graph::open(root)?;
    let q = graph.query();

    // Seeds for PPR: every node whose `name` matches a query token.
    let mut seeds: Vec<i64> = Vec::new();
    if let Some(query) = request.query.as_deref() {
        for raw in query.split(|c: char| !c.is_alphanumeric() && c != '_') {
            if raw.len() < 2 {
                continue;
            }
            for n in q.find(raw, 4)? {
                seeds.push(n.id);
            }
        }
    }
    let bridges = q.bridge_scores()?;
    let pool: Vec<i64> = graph.store.all_node_ids()?;
    let pp = if seeds.is_empty() {
        // No name match → use plain PageRank as the centrality baseline.
        q.top_central(&pool, pool.len())?
            .into_iter()
            .collect::<Vec<_>>()
    } else {
        q.top_central_personalized(&seeds, &pool, pool.len())?
    };
    let pp_map: std::collections::HashMap<i64, f64> = pp.into_iter().collect();

    // Map node-id scores back to chunk-id keys via path matching. Each
    // chunk gets the *max* score across nodes that live in its path —
    // representative of "the most central thing this chunk discusses".
    let mut priors: Priors = Priors::new();
    for chunk in &request.chunks {
        let path = match chunk.path.as_deref() {
            Some(p) => p,
            None => continue,
        };
        let mut best = 0.0f64;
        for n in graph.store.nodes_in_file(path)? {
            let pp_score = pp_map.get(&n.id).copied().unwrap_or(0.0);
            let bridge_score = bridges.get(n.id);
            let combined = pp_score + 0.5 * bridge_score;
            if combined > best {
                best = combined;
            }
        }
        if best > 0.0 {
            priors.insert(chunk.id.clone(), best);
        }
    }
    Ok(priors)
}

fn tag_chunk_communities(root: &Path, request: &mut OptimizationRequest) -> Result<()> {
    let graph = Graph::open(root)?;
    let communities = graph.query().communities()?;
    for chunk in &mut request.chunks {
        if chunk.community.is_some() {
            continue; // caller-supplied label takes precedence
        }
        let path = match chunk.path.as_deref() {
            Some(p) => p,
            None => continue,
        };
        // Pick the most common community among nodes in this file.
        let mut counts: std::collections::HashMap<u32, usize> = std::collections::HashMap::new();
        for n in graph.store.nodes_in_file(path)? {
            if let Some(c) = communities.community_of(n.id) {
                *counts.entry(c).or_insert(0) += 1;
            }
        }
        if let Some((dominant, _)) = counts.into_iter().max_by_key(|(_, n)| *n) {
            chunk.community = Some(dominant);
        }
    }
    Ok(())
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

fn run_impact(
    root: &Path,
    depth: u32,
    strategy: &str,
    prune_unreachable: bool,
    files: Vec<String>,
) -> Result<()> {
    let graph = Graph::open(root)?;

    // Resolve seed node ids — every node in each named file. Strategy
    // dispatch happens below; default `bfs` is byte-identical to the
    // pre-Phase-2 implementation.
    let mut seed_ids: Vec<i64> = Vec::new();
    for p in &files {
        for n in graph.store.nodes_in_file(p)? {
            seed_ids.push(n.id);
        }
    }

    let mut impacted_ids: Vec<i64> = match strategy {
        "bfs" => {
            let result = graph.query().impact_radius(&files, depth)?;
            result.impacted.iter().map(|n| n.id).collect()
        }
        "rwr" => {
            // RWR yields scored ids; we keep the top probability mass
            // matching what BFS would have returned in size order. We
            // fall back to all positive-mass nodes if the BFS would
            // have been smaller.
            let scored = graph.query().impact_rwr(&seed_ids, 1024)?;
            scored.into_iter().map(|(id, _)| id).collect()
        }
        "steiner" => {
            let result = graph.query().steiner_subgraph(&seed_ids)?;
            result.nodes
        }
        other => anyhow::bail!("unknown impact strategy: {other}"),
    };

    if prune_unreachable {
        let live = graph
            .query()
            .reachable_from(&seed_ids)?
            .reachable
            .into_iter()
            .collect::<std::collections::HashSet<_>>();
        impacted_ids.retain(|id| live.contains(id));
    }

    let mut paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    let mut node_count = 0usize;
    for id in &impacted_ids {
        if let Some(n) = graph.store.node(*id)? {
            paths.insert(n.path);
            node_count += 1;
        }
    }
    for p in &paths {
        println!("{p}");
    }
    eprintln!(
        "impact: strategy={strategy} seeds={} impacted_nodes={} unique_files={} pruned_unreachable={prune_unreachable}",
        seed_ids.len(),
        node_count,
        paths.len(),
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

/// Single-command setup: index the repo and wire Claude Code in one shot.
/// Used by `install.sh` so `curl … | bash` from inside a project directory
/// leaves the user with a fully-functional setup, no follow-up commands.
fn run_init(root: &Path, skip_build: bool) -> Result<()> {
    if !skip_build {
        eprintln!("ContextOS init [1/2] indexing repo at {}…", root.display());
        let graph = Graph::open(root)?;
        let report = graph.builder().build()?;
        eprintln!(
            "  built: scanned={} reparsed={} nodes={} edges={}",
            report.files_scanned,
            report.files_reparsed,
            report.nodes_written,
            report.edges_written,
        );
    } else {
        eprintln!("ContextOS init [1/2] skipping build (--skip-build)");
    }
    eprintln!("ContextOS init [2/2] wiring Claude Code…");
    let report = install::install(root)?;
    if report.already_configured {
        eprintln!("  Claude Code already configured for this project — no changes.");
    } else {
        eprintln!("  wrote {}", report.mcp_json_path.display());
        eprintln!("  wrote {}", report.settings_path.display());
    }
    // Auto-update .gitignore so per-project ContextOS state never lands in
    // a commit by accident. Creates the file if it doesn't exist; appends
    // to it if it does, idempotently.
    match install::ensure_gitignore(root) {
        Ok(true) => eprintln!(
            "  updated .gitignore (added .mcp.json, .claude/, .contextos/)"
        ),
        Ok(false) => {}
        Err(e) => eprintln!("  .gitignore update skipped: {e}"),
    }
    eprintln!();
    eprintln!("Done. Open Claude Code in this project and ContextOS will be active.");
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

// ---- savings dashboard --------------------------------------------------

/// ANSI helpers. Output is written via `println!` but each colored
/// segment is wrapped explicitly so we can disable on `--no-color`.
struct Style {
    enabled: bool,
}

impl Style {
    fn paint(&self, code: &str, s: &str) -> String {
        if self.enabled {
            format!("\x1b[{code}m{s}\x1b[0m")
        } else {
            s.to_string()
        }
    }
    fn cyan(&self, s: &str) -> String { self.paint("36", s) }
    fn green(&self, s: &str) -> String { self.paint("32", s) }
    fn yellow(&self, s: &str) -> String { self.paint("33", s) }
    fn red(&self, s: &str) -> String { self.paint("31", s) }
    fn bold(&self, s: &str) -> String { self.paint("1", s) }
    fn dim(&self, s: &str) -> String { self.paint("2", s) }
    fn header(&self, label: &str) -> String {
        let inner = format!(" {label} ");
        if self.enabled {
            format!("\x1b[7;36m{inner}\x1b[0m")
        } else {
            format!("[ {label} ]")
        }
    }
}

fn run_savings(top: usize, project_filter: Option<&Path>, no_color: bool) -> Result<()> {
    let s = Style {
        enabled: !no_color && atty_stdout(),
    };
    let records = contextos_utils::read_usage();
    let project_filter_canon = project_filter
        .and_then(|p| p.canonicalize().ok())
        .map(|p| p.to_string_lossy().into_owned());

    let filtered: Vec<&contextos_utils::UsageRecord> = records
        .iter()
        .filter(|r| match (&project_filter_canon, &r.project) {
            (Some(want), Some(got)) => got == want,
            (Some(_), None) => false,
            _ => true,
        })
        .collect();

    if filtered.is_empty() {
        let scope = if project_filter.is_some() {
            "this project"
        } else {
            "all projects"
        };
        println!("{}", s.header("ContextOS Token Savings"));
        println!();
        println!(
            "  No usage data for {scope} yet. Run a few `contextos optimize` calls\n  (or use ContextOS via Claude Code's MCP), then check back."
        );
        return Ok(());
    }

    let total_count = filtered.len();
    let total_in: usize = filtered.iter().map(|r| r.in_tokens).sum();
    let total_out: usize = filtered.iter().map(|r| r.out_tokens).sum();
    let total_saved: usize = filtered.iter().map(|r| r.saved_tokens).sum();
    let total_elapsed_ms: f64 = filtered.iter().map(|r| r.elapsed_ms).sum();
    let avg_elapsed_ms = total_elapsed_ms / total_count as f64;
    let pct = if total_in == 0 {
        0.0
    } else {
        (total_saved as f64 / total_in as f64) * 100.0
    };

    let scope_label = if project_filter.is_some() {
        "Project Scope"
    } else {
        "Global Scope"
    };
    println!(
        "{}",
        s.header(&format!("ContextOS Token Savings ({scope_label})"))
    );
    println!();
    println!("  Total commands:    {}", s.bold(&format!("{total_count}")));
    println!("  Input tokens:      {}", humanize(total_in));
    println!("  Output tokens:     {}", humanize(total_out));
    println!(
        "  Tokens saved:      {} ({})",
        s.bold(&humanize(total_saved)),
        color_pct(&s, pct)
    );
    println!(
        "  Total exec time:   {} (avg {})",
        humanize_ms(total_elapsed_ms),
        humanize_ms(avg_elapsed_ms)
    );
    println!(
        "  Efficiency meter:  {}",
        meter_bar(&s, pct, 30)
    );
    println!();
    println!("{}", s.header("By Command"));
    println!();

    // Aggregate by query (or "(no query)" bucket).
    use std::collections::HashMap;
    #[derive(Default)]
    struct Agg {
        count: usize,
        saved: usize,
        in_tokens: usize,
        elapsed_ms: f64,
    }
    let mut by_query: HashMap<String, Agg> = HashMap::new();
    for r in &filtered {
        let key = r
            .query
            .as_deref()
            .map(|q| q.trim().to_string())
            .filter(|q| !q.is_empty())
            .unwrap_or_else(|| "(no query)".into());
        let entry = by_query.entry(key).or_default();
        entry.count += 1;
        entry.saved += r.saved_tokens;
        entry.in_tokens += r.in_tokens;
        entry.elapsed_ms += r.elapsed_ms;
    }
    let mut rows: Vec<(String, Agg)> = by_query.into_iter().collect();
    rows.sort_by(|a, b| b.1.saved.cmp(&a.1.saved));
    let max_saved = rows.iter().map(|(_, a)| a.saved).max().unwrap_or(0).max(1);

    println!(
        "  {:>3}  {:<32}  {:>5}  {:>7}  {:>6}  {:>7}  {}",
        "#", "Command", "Count", "Saved", "Avg%", "Time", "Impact"
    );
    println!("  {}", s.dim(&"─".repeat(78)));
    for (i, (q, a)) in rows.iter().take(top).enumerate() {
        let avg = if a.in_tokens == 0 {
            0.0
        } else {
            (a.saved as f64 / a.in_tokens as f64) * 100.0
        };
        let cmd_short = truncate(q, 32);
        let bar = impact_bar(&s, a.saved, max_saved, 12);
        println!(
            "  {:>3}  {:<32}  {:>5}  {:>7}  {:>6}  {:>7}  {}",
            i + 1,
            cmd_short,
            a.count,
            humanize(a.saved),
            color_pct(&s, avg),
            humanize_ms(a.elapsed_ms),
            bar,
        );
    }
    println!();
    Ok(())
}

fn humanize(n: usize) -> String {
    if n >= 1_000_000 {
        format!("{:.1}M", n as f64 / 1_000_000.0)
    } else if n >= 1_000 {
        format!("{:.1}K", n as f64 / 1_000.0)
    } else {
        n.to_string()
    }
}

fn humanize_ms(ms: f64) -> String {
    if ms >= 1_000.0 {
        format!("{:.1}s", ms / 1_000.0)
    } else {
        format!("{:.0}ms", ms)
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let cut: String = s.chars().take(max.saturating_sub(1)).collect();
    format!("{cut}…")
}

fn color_pct(s: &Style, pct: f64) -> String {
    let txt = format!("{pct:.1}%");
    if pct >= 60.0 {
        s.green(&txt)
    } else if pct >= 30.0 {
        s.yellow(&txt)
    } else {
        s.red(&txt)
    }
}

fn meter_bar(s: &Style, pct: f64, width: usize) -> String {
    let pct_clamped = pct.clamp(0.0, 100.0);
    let filled = ((pct_clamped / 100.0) * width as f64).round() as usize;
    let empty = width.saturating_sub(filled);
    let bar = format!(
        "{}{}",
        "█".repeat(filled),
        s.dim(&"░".repeat(empty)),
    );
    format!("{}  {}", s.green(&bar), color_pct(s, pct))
}

fn impact_bar(s: &Style, saved: usize, max: usize, width: usize) -> String {
    let filled = if max == 0 {
        0
    } else {
        ((saved as f64 / max as f64) * width as f64).round() as usize
    };
    let empty = width.saturating_sub(filled);
    format!("{}{}", s.cyan(&"▆".repeat(filled)), s.dim(&"·".repeat(empty)))
}

fn atty_stdout() -> bool {
    // Cheap, dependency-free isatty check via libc, gated to unix.
    #[cfg(unix)]
    {
        // SAFETY: isatty(fd) is a thread-safe syscall with no side effects.
        unsafe { libc_isatty(1) != 0 }
    }
    #[cfg(not(unix))]
    {
        true
    }
}

#[cfg(unix)]
extern "C" {
    fn isatty(fd: i32) -> i32;
}

#[cfg(unix)]
unsafe fn libc_isatty(fd: i32) -> i32 {
    unsafe { isatty(fd) }
}

// ---- phase 2 graph commands -------------------------------------------

fn resolve_seeds_by_path(graph: &Graph, paths: &[String]) -> Result<Vec<i64>> {
    let mut ids = Vec::new();
    for p in paths {
        for n in graph.store.nodes_in_file(p)? {
            ids.push(n.id);
        }
    }
    Ok(ids)
}

fn run_rwr(root: &Path, top_k: usize, seeds: Vec<String>) -> Result<()> {
    let graph = Graph::open(root)?;
    let seed_ids = resolve_seeds_by_path(&graph, &seeds)?;
    if seed_ids.is_empty() && !seeds.is_empty() {
        eprintln!("rwr: no graph nodes match the provided seed paths — run `contextos build` first?");
    }
    let scored = graph.query().impact_rwr(&seed_ids, top_k)?;
    for (id, score) in scored {
        if let Some(n) = graph.store.node(id)? {
            println!("{:.6}\t{}\t{}", score, n.path, n.qualified);
        }
    }
    Ok(())
}

fn run_communities(root: &Path) -> Result<()> {
    let graph = Graph::open(root)?;
    let result = graph.query().communities()?;
    eprintln!(
        "communities: count={} modularity={:.4}",
        result.count, result.modularity
    );
    let mut entries: Vec<(u32, String, String)> = Vec::with_capacity(result.of.len());
    for (&id, &cid) in &result.of {
        if let Some(n) = graph.store.node(id)? {
            entries.push((cid, n.path, n.qualified));
        }
    }
    entries.sort_unstable();
    for (cid, path, qual) in entries {
        println!("{cid}\t{path}\t{qual}");
    }
    Ok(())
}

fn run_bridges(root: &Path, top_k: usize) -> Result<()> {
    let graph = Graph::open(root)?;
    let r = graph.query().bridge_scores()?;
    let mut entries: Vec<(i64, f64)> = r.scores.into_iter().collect();
    entries.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
    for (id, score) in entries.into_iter().take(top_k) {
        if let Some(n) = graph.store.node(id)? {
            println!("{:.4}\t{}\t{}", score, n.path, n.qualified);
        }
    }
    Ok(())
}

fn run_steiner(root: &Path, names: Vec<String>) -> Result<()> {
    let graph = Graph::open(root)?;
    let mut terminals: Vec<i64> = Vec::new();
    for name in &names {
        for n in graph.query().find(name, 8)? {
            terminals.push(n.id);
        }
    }
    if terminals.is_empty() {
        eprintln!("steiner: no symbols matched the provided names");
        std::process::exit(2);
    }
    let result = graph.query().steiner_subgraph(&terminals)?;
    for id in &result.nodes {
        if let Some(n) = graph.store.node(*id)? {
            println!("{}\t{}", n.path, n.qualified);
        }
    }
    eprintln!(
        "steiner: terminals={} nodes={} edges={}",
        terminals.len(),
        result.nodes.len(),
        result.edges.len()
    );
    Ok(())
}

fn run_reachable(root: &Path, files: Vec<String>) -> Result<()> {
    let graph = Graph::open(root)?;
    let roots = resolve_seeds_by_path(&graph, &files)?;
    let r = graph.query().reachable_from(&roots)?;
    let mut paths: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    for id in r.reachable {
        if let Some(n) = graph.store.node(id)? {
            paths.insert(n.path);
        }
    }
    for p in paths {
        println!("{p}");
    }
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
