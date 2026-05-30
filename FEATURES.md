# ContextOS — Feature List

A complete inventory of what ContextOS does today. Treat this as the
single source of truth for capabilities; the [README](README.md)
covers philosophy, [ARCHITECTURE.md](ARCHITECTURE.md) the internals,
and [USAGE.md](USAGE.md) day-to-day workflows.

> **Current version:** 0.3.1
> **Platforms:** macOS (arm64 + x86_64), Linux (x86_64)
> **Runtime cost:** single static binary, ~8 MB, no Python / Node / native deps.

---

## 1. Token-Reduction Engine

The core optimisation pipeline runs end-to-end in a few milliseconds per
call. Every stage is optional and reports per-stage stats.

| Stage | What it does |
|---|---|
| **Skeletonisation** | Replaces low-priority chunks with signature-only views (function/class declarations, no bodies). Driven by tree-sitter on Rust / TypeScript / JavaScript / Python. |
| **Dedup** | Drops byte-identical chunks (exact hash) and near-duplicates via 64-bit **SimHash** (Hamming threshold ≤ 3) plus **MinHash-LSH** for larger corpora. Falls back to pairwise Jaccard when LSH isn't worth setting up. |
| **AST-aware compression** | Strips comments, debug logs, and empty lines. Uses tree-sitter when available, regex fallback otherwise. Cross-chunk **boilerplate detection** via Count-Min sketch — recurring license headers / copyright banners are collapsed to a single occurrence. |
| **Ranking** | **BM25** + optional **RM3 query expansion**; fused with graph priors (Personalized PageRank seeded by query terms, plus sampled betweenness centrality) via Reciprocal Rank Fusion when a graph is available. |
| **Budget allocator** | **MMR submodular** greedy with optional **Louvain community-balanced** coverage bonus. Honours a `max_tokens` cap and a configurable `lambda` relevance/diversity knob. |
| **Caller priors** | Pluggable score bumps for "active file", "selected region", "file under cursor". |

### Supported languages (for AST-aware stripping)
Rust, TypeScript, TSX, JavaScript (incl. JSX, MJS, CJS), Python, Go,
Java (incl. Kotlin / Scala), Ruby, C, C++ (incl. headers), JSON,
Markdown. Unknown languages still benefit from whitespace
normalisation and empty-line removal.

---

## 2. Code Graph (`.contextos/graph.db`)

A SQLite-backed structural model of the repo, built once and updated
incrementally.

- **Tree-sitter parsing** for every supported language.
- **23+ node and edge kinds** (function, class, module, file, calls,
  imports, inherits, tests, references, …).
- **SHA-256 incremental indexing** — only re-parses files whose content
  has actually changed.
- **File-system watcher** (`contextos watch`) keeps the graph fresh on
  every save.
- **Graph queries:**
  - `impact_radius(files, depth)` — blast radius for a changeset.
  - `skeleton_for(path)` — signature-only view.
  - `personalized_pagerank(seeds)` — relevance priors for the engine.
  - `random_walk_with_restart(seeds)` — `rwr` CLI subcommand.
  - `louvain_communities()` — `communities` subcommand.
  - `betweenness(sampled)` — `bridges` subcommand.
  - `steiner_subgraph(terminals)` — `steiner` subcommand.
  - `forward_reachable(roots)` — `reachable` subcommand.

---

## 3. CLI Commands

Single binary, 16 subcommands.

### Indexing
- `build` — full graph build (or refresh) for `--root`.
- `update <files>` — incremental re-index of just the listed files.
- `watch` — auto-update on save.
- `stats` — node / edge / file counts.

### Queries
- `impact <files>` — blast radius for a changeset.
- `skeleton <path>` — signature-only view of a file.
- `rwr --seeds=… --top=…` — random-walk-with-restart.
- `communities` — Louvain partition (one community per line).
- `bridges` — sampled betweenness centrality.
- `steiner --terminals=…` — approximate Steiner subgraph connecting a
  set of symbols.
- `reachable --roots=…` — forward-reachable closure.

### Engine
- `optimize --input=… --output=…` — run the pipeline on supplied JSON
  chunks. Supports `--max-tokens`, `--graph-root` (enables PPR /
  betweenness / Louvain priors), `--rm3`, `--pretty`.

### Claude Code integration
- `init [--strict] [--skip-build]` — one-shot setup: build the graph
  and wire Claude Code in one command.
- `install [--strict]` — just write the wiring files (no build).
- `uninstall` — remove ContextOS entries from `.mcp.json` and
  `.claude/settings.local.json`.
- `hook <kind>` — internal entrypoint used by Claude Code's
  `PreToolUse` hook when strict mode is enabled.

### Misc
- `serve --root=…` — run as an MCP JSON-RPC server on stdio (this is
  what Claude Code spawns).
- `savings` — colored ASCII dashboard of cumulative token reductions.
- `version`, `help` — standard.

---

## 4. Claude Code MCP Server

Exposed when Claude Code launches the binary via `contextos serve`.

| Tool | Purpose |
|---|---|
| `optimize` | Run the full pipeline on supplied code chunks. Honours `max_tokens` and `query`. |
| `cx_pack_files` | Read N files from disk, rank against a `query`, return a budget-trimmed packed view with per-file headers, **coverage / confidence score**, and the list of dropped paths so the LLM can re-fetch if needed. |
| `skeleton` | Signature-only view of one file. Records a `UsageRecord` so the saving shows up in `/savings`. |
| `build_graph` | Build or refresh the graph for the active repo. |
| `update_graph` | Incremental update for a list of files. |
| `impact_radius` | Blast radius for a changeset. |
| `graph_stats` | Node / edge / file counts. |
| `savings` | Returns the savings dashboard as Markdown. |

Tools advertise descriptions that nudge the LLM toward the right tool
("Prefer this over Read when you only need the structure of a large
file"), so even without strict mode they get picked up.

---

## 5. Strict Enforcement (`--strict`)

Opt-in `PreToolUse` hooks registered in `.claude/settings.json`. The
hook process is the same `contextos` binary, called as
`contextos hook <kind>`. Always fails open on error — a buggy hook
never wedges Claude Code's tool flow.

| Hook | Behaviour |
|---|---|
| `pre-read` | Allows non-`Read` tools, paginated reads (`offset`/`limit`), and files ≤ 300 lines. Blocks `Read` on larger files with a reason that hands the model the exact `mcp__contextos__skeleton` / `cx_pack_files` call to retry with. |
| `pre-grep` | Allows `output_mode=files_with_matches`/`count`, bounded `head_limit ≤ 200`, and single-file searches. Blocks unbounded repo-wide `output_mode=content` Greps with a reason listing the four retry options (narrow the pattern, add `head_limit`, scope with `path`/`glob`, or hand off to `cx_pack_files`). |

Both gates are idempotent and support partial upgrades (an existing
install can be upgraded in place to add a missing matcher without
disturbing the others).

---

## 6. Install & Setup

Run `contextos init` from a project root and it does all of:

- Writes `.mcp.json` (registers the MCP server).
- Writes `.claude/settings.local.json` (opts the project into the
  MCP server when `enabledMcpjsonServers` gating is on).
- Updates `.gitignore` to ignore `.mcp.json`, `.claude/`, and
  `.contextos/` (only adds lines that aren't already present).
- Installs the user-scoped `/savings` slash command at
  `~/.claude/commands/savings.md`.
- Refreshes a fenced **ContextOS guidance block** in the project's
  `CLAUDE.md` that nudges Claude to prefer `skeleton` /
  `cx_pack_files` over raw `Read` (idempotent — fenced with HTML
  markers so re-runs upgrade in place without touching user content).
- With `--strict`: registers the `PreToolUse` hooks for `Read` and
  `Grep` (above).

The companion `infra/scripts/install.sh` provides a one-liner remote
install:

```bash
curl -fsSL https://raw.githubusercontent.com/DhiravPatel/ContextOS/main/infra/scripts/install.sh | bash
```

Supports env-var overrides: `CONTEXTOS_VERSION`, `CONTEXTOS_NO_INIT`,
`CONTEXTOS_SKIP_BUILD`, `CONTEXTOS_INSTALL_DIR`.

---

## 7. Savings Dashboard (`/savings`)

Local, file-backed analytics — never leaves the machine.

Every `optimize`, `cx_pack_files`, and `skeleton` call appends one
JSONL record to `~/.contextos/usage.jsonl`. The `/savings` slash
command (or `contextos savings`) renders a dashboard from it.

**What it shows:**
- **Tokens saved** vs. tokens of original input vs. output tokens.
- **Estimated $ saved** — multiplies tokens × per-million input price.
  Defaults to Sonnet ($3 / M); selectable via
  `CONTEXTOS_PRICING_MODEL=opus|sonnet|haiku` or a custom rate via
  `CONTEXTOS_INPUT_PRICE_PER_M=<float>`.
- **Aggregate reduction %** (cumulative) and **per-call mean %**.
- **Total exec time** + average per call + call count.
- **Top commands** table — grouped by `query`, sorted by tokens saved.
- **By user** table (only renders when ≥ 1 record has a user tag) —
  per-developer breakdown for shared-account scenarios.

**Scopes:**
- `global` (default) — every record across every project on this
  machine.
- `project` — filtered to the active project root.

**Per-call record fields:** `ts`, `in_tokens`, `out_tokens`,
`saved_tokens`, `elapsed_ms`, `query`, `chunks_in`, `chunks_out`,
`source` (cli / mcp / mcp.cx_pack_files / mcp.skeleton), `project`,
`user`.

Disable telemetry entirely with `CONTEXTOS_NO_USAGE=1`.

---

## 8. Per-User Attribution (Phase 1)

For teams sharing one Claude Max / Claude Code account.

- Every record is auto-stamped with a `user` field at write time.
- Identity resolved in order: `$CONTEXTOS_USER` → `git config
  user.email` → `$USER` → `whoami`.
- `/savings` adds a "BY USER" section listing calls / saved / avg-%
  per teammate when the log contains tagged records.

Phase 2 (push records to a shared backend so the whole team gets a
single dashboard) is designed but not yet implemented.

---

## 9. Confidence & Coverage on `cx_pack_files`

When the tool drops files to fit the budget, the response includes:

- A human-readable line: `Coverage: HIGH|PARTIAL|LOW · N/M files kept
  (X% files, Y% tokens)`.
- A trailing JSON block with `files_kept`, `files_total`,
  `file_coverage`, `token_coverage`, `confidence`, and
  `dropped_paths`.

Lets the model decide deterministically whether the packed view
suffices or whether to re-fetch a dropped file via `Read`.

---

## 10. Privacy & Local-First Guarantees

- **No network calls.** Ever. The binary doesn't open sockets.
- **No external dependencies.** Single static binary; no Python, Node,
  Java, embeddings server.
- **All state on disk under `$HOME`:**
  - `~/.local/bin/contextos` — binary
  - `~/.contextos/usage.jsonl` — telemetry log
  - `~/.claude/commands/savings.md` — slash command
  - `<project>/.contextos/graph.db` — per-project graph
  - `<project>/.mcp.json`, `<project>/.claude/*.json` — per-project
    wiring
- **Telemetry is content-free.** Records token counts, the query
  string (which the user typed) and the project root — never source
  bytes. Disable with `CONTEXTOS_NO_USAGE=1`.

---

## 11. Slash Commands

User-scoped, installed at `~/.claude/commands/`:

- **`/savings`** — invokes the MCP `savings` tool, displays the
  dashboard inline, appends a one-line takeaway. Falls back to a
  helpful "run `contextos init`" message if the MCP server isn't
  wired in the active project.

---

## 12. Quality / Reliability

- **Test coverage:** 112+ unit tests across the workspace (engine
  pipeline stages, graph queries, MCP tool handlers, hook gates,
  install / upgrade flows, savings rendering, dollar conversion).
- **Failure handling:** every hook fails open; every UsageRecord
  write is non-fatal; every install action is idempotent and
  partially-upgradeable.
- **Performance budget:** end-to-end optimize latency typically
  < 50 ms for inputs up to ~30k tokens; sub-millisecond for hooks.
- **Reproducible savings record:** every save is logged with
  pre-computed `saved_tokens` so consumers don't recompute.

---

## 13. Configuration Cheat Sheet

| Env var | Purpose | Default |
|---|---|---|
| `CONTEXTOS_NO_USAGE` | Disable telemetry log writes | unset (logging on) |
| `CONTEXTOS_USER` | Identity tag for usage records | git email / `$USER` / `whoami` |
| `CONTEXTOS_PRICING_MODEL` | `opus` / `sonnet` / `haiku` for `/savings` dollar row | `sonnet` |
| `CONTEXTOS_INPUT_PRICE_PER_M` | Custom per-million-token rate (USD, float) | — |
| `CONTEXTOS_INSTALL_DIR` | Install location for `install.sh` | `~/.local/bin` |
| `CONTEXTOS_VERSION` | Pin a specific release for `install.sh` | latest |
| `CONTEXTOS_NO_INIT` | `install.sh` only installs the binary; skip wiring | unset |
| `CONTEXTOS_SKIP_BUILD` | `install.sh` wires up but defers graph build | unset |

---

## 14. Known Limitations (Honest List)

- **SimHash dedup is strict** (3-bit Hamming) — catches byte-near
  duplicates but not semantic ones (e.g., renamed identifiers).
  Semantic dedup via local embeddings is on the roadmap.
- **Compression is no-op on tiny inputs.** With ≤ 1 chunk and no
  cross-chunk boilerplate, there's nothing for `compress` to do.
- **Budget-driven savings dominate over compression-driven savings**
  on real Claude Code traffic. That's expected: relevance ranking is
  the bigger lever.
- **Strict mode only currently gates `Read` and `Grep`.** Other
  tools (Bash output, WebFetch) are not yet intercepted.
- **Team rollup is single-machine only today** — per-user attribution
  works locally but the cross-machine sync layer (Phase 2) is not yet
  shipped.

---

Want a feature that isn't on this list? Open an issue at
<https://github.com/DhiravPatel/ContextOS/issues> or add a row to the
roadmap.
