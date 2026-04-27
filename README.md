# ContextOS

> Local-first, lossless token reduction for AI coding assistants. Graph-aware context selection + mathematically principled compression. Cut prompt tokens **≥50%** (often much more) without changing LLM output quality.

AI assistants (Cursor, Copilot, Claude Code, Cody) burn tokens by shoving whole repos into every prompt. ContextOS sits in front as a **pre-processor + context engine**: build a structural graph of your code, compute the minimum relevant slice, then run an AST-aware compression pipeline on top.

---

## Why this works

Token reduction without quality loss comes from two orthogonal ideas:

1. **Don't send what isn't needed.** A code graph (built by Tree-sitter, stored in SQLite) lets us compute **blast radius** — given a changed file, which files actually depend on it? Everything else is noise. This is the graph layer.
2. **Compress what you *do* send.** Remove redundancy (dedup), remove non-semantic text (comments, debug logs), pack within a budget by relevance (BM25 + PageRank). This is the pipeline layer.

Both layers are lossless: they drop things the LLM doesn't need, never paraphrase or summarise what it does.

---

## Architecture

```
              repo on disk
                   │
                   ▼
          ┌──────────────────┐
          │  Graph Builder   │  Tree-sitter → SQLite
          │  (.contextos/    │  SHA-256 incremental
          │   graph.db)      │  23+ node/edge types
          └────────┬─────────┘
                   │
          ┌────────▼──────────────────────────┐
          │  Graph Queries                    │
          │  • blast_radius(files, depth)     │
          │  • skeleton_for(path)             │
          │  • pagerank → priors              │
          └────────┬──────────────────────────┘
                   │    only the chunks that matter
                   ▼
          ┌───────────────────────────────────┐
          │  Core Engine Pipeline             │
          │  1. Skeletonise (signature-only)  │
          │  2. Dedup (exact + MinHash-LSH)   │
          │  3. Compress (AST strip, parallel)│
          │  4. Rank (BM25 + PageRank priors) │
          │  5. Budget (greedy within cap)    │
          └────────┬──────────────────────────┘
                   │
                   ▼
          optimised bundle → your LLM
```

Exposed to AI clients through:

- **CLI** (`contextos` binary) — stdin/stdout JSON
- **MCP server** (`contextos serve`) — JSON-RPC 2.0 over stdio; works with Claude Code, Cursor, any MCP-speaking client
- **VS Code extension** — one-click `⌘⌥O`

---

## The math under the hood

Every algorithm below preserves output semantics — nothing is rephrased, summarised, or renamed.

| Technique | Purpose | Complexity |
|---|---|---|
| **SHA-256 file hashing** | Incremental graph updates (skip unchanged files) | O(file bytes) |
| **Tree-sitter AST walk** | Syntax-aware comment/log stripping (no string-literal collisions) | O(source bytes) |
| **Jaccard over line fingerprints** | Near-dup detection for small chunk sets | O(n²) — small n only |
| **MinHash + LSH** (128 permutations, 16 bands × 8 rows) | Scalable near-dup detection for repo-scale inputs | O(n · perm) construction, O(n) candidate lookup |
| **Okapi BM25** (k1=1.5, b=0.75) | Length-normalised query-to-chunk relevance | O(chunks · query terms) |
| **TF-IDF density** | Fallback ranker when no user query | O(total tokens) |
| **PageRank** (damping=0.85, power iteration, tol=1e-6) | Centrality prior: structurally important symbols win ties | O(iters · edges), ~10ms for 10k nodes |
| **BFS blast radius** | Reverse-edge traversal over `calls ∪ imports ∪ inherits` | O(impacted nodes + edges) |
| **Greedy knapsack** (with 5% slack) | Pack highest-ranked chunks into `max_tokens` | O(n) post-ranking |

Typical combined reduction on a real TypeScript service: **73%** on redundant-file workloads, **88%** when the graph picks only the blast radius and the pipeline compresses what's left.

---

## Repository layout

```
ContextOS/
├── Cargo.toml                       # Rust workspace
├── package.json                     # npm workspace (extension only)
├── apps/
│   ├── cli/                         # `contextos` binary
│   │   └── src/{main.rs, mcp.rs, watch.rs}
│   └── extension/                   # VS Code extension (TypeScript)
├── crates/
│   ├── utils/                       # hashing, tokenising, language detection
│   ├── tokenizer/                   # BPE-approximating token estimator
│   ├── parser/                      # tree-sitter strip with regex fallback
│   ├── graph/                       # code graph (builder, store, query, pagerank)
│   │   └── src/{builder.rs, store.rs, query.rs, pagerank.rs, types.rs}
│   └── core-engine/                 # optimisation pipeline
│       └── src/
│           ├── skeleton/            # signature-only view
│           ├── dedup/               # exact + MinHash-LSH
│           ├── compress/            # AST-aware strip (parallel)
│           ├── ranking/             # BM25 + TF-IDF + PageRank priors
│           └── budget/              # greedy knapsack
├── infra/
│   ├── docker/Dockerfile            # CLI-only container
│   └── scripts/build.sh             # one-shot full build
└── docs/
    ├── ARCHITECTURE.md
    ├── USAGE.md
    ├── ALGORITHMS.md                # math details for each technique
    ├── DEPLOYMENT.md                # full release flow + end-user install + E2E flow
    └── PUBLISHING.md                # step-by-step VS Code Marketplace + Open VSX publish
```

---

## Install (terminal, no editor extension needed)

ContextOS ships as a single static binary. After install it works with **Claude Code** out of the box via MCP — no VS Code marketplace, no Node, no Python.

### One command, fully wired

From inside the project you want to use it in:

```bash
cd /path/to/your/repo
curl -fsSL https://raw.githubusercontent.com/DhiravPatel/ContextOS/main/infra/scripts/install.sh | bash
```

That single command:

1. Detects your OS + architecture (macOS arm64/x64, Linux x64/arm64).
2. Downloads the matching `contextos` binary from the latest GitHub Release.
3. Verifies the SHA-256 checksum against `SHA256SUMS` in the same release.
4. Drops it into `~/.local/bin/contextos` (override with `CONTEXTOS_INSTALL_DIR=...`).
5. Detects you're inside a project (looks for `.git`, `package.json`, `Cargo.toml`, etc.) and runs `contextos init` for you — builds the graph **and** writes `.mcp.json` for Claude Code.

After that finishes, open Claude Code in this project and ContextOS is already wired up. No follow-up commands.

### Adding a second project later

The binary is installed once; only the per-project wiring needs to be repeated:

```bash
cd /path/to/another/repo
contextos init
```

That's `build + install` in one shot.

### Seeing how much you've saved

ContextOS records every `optimize` call (CLI or MCP) into an append-only log at `~/.contextos/usage.jsonl`. Show the dashboard with:

```bash
contextos savings              # global scope, all projects
contextos savings --project .  # only this project
contextos savings --top 20     # show 20 rows in the by-command table
contextos savings --no-color   # plain output (for redirection / CI)
```

Output is a colored ASCII summary: total tokens in/out, cumulative savings, average reduction %, total exec time, plus a per-query breakdown with an impact bar. The log only stores token counts, the user's query string, and the project root — no source code ever leaves the file. Disable telemetry with `CONTEXTOS_NO_USAGE=1`.

### Useful overrides

```bash
CONTEXTOS_VERSION=v0.2.1 curl … | bash      # pin a release
CONTEXTOS_NO_INIT=1     curl … | bash       # install binary only, no auto-wire
CONTEXTOS_SKIP_BUILD=1  curl … | bash       # auto-wire but defer graph build
CONTEXTOS_INSTALL_DIR=/usr/local/bin curl … | bash
```

### Windows

Download `contextos-win32-x64.zip` from the [latest release](https://github.com/DhiravPatel/ContextOS/releases/latest), extract, and add the folder to `PATH`. Then:

```powershell
cd C:\path\to\your\repo
contextos init
```

### Build from source

```bash
# Prerequisites: Rust stable, Node.js ≥18 (only needed for the optional VS Code extension)
./infra/scripts/build.sh
./target/release/contextos --help
```

---

## Quickstart (after install)

```bash
# Index your repo
contextos build --root /path/to/your/repo

# See the blast radius of changed files
git diff --name-only | xargs contextos impact --root /path/to/your/repo

# Run as a live MCP server (wire to Claude Code / Cursor config)
contextos serve --root /path/to/your/repo

# Or pipe raw JSON through the pipeline (no graph needed)
echo '{"chunks":[{"id":"a","language":"typescript","content":"// hi\nfn add(){}","kind":"code","priority":0,"skeleton_hint":false}],"query":"addition"}' \
  | contextos optimize --pretty
```

---

## CLI reference

| Subcommand | Purpose |
|---|---|
| `contextos build --root <path>` | Full repo graph build (respects .gitignore) |
| `contextos update --root <path> [files...]` | Incremental update; pipes from `git diff --name-only` |
| `contextos impact --root <path> --depth N <files...>` | Print blast radius for the given changed files |
| `contextos skeleton --root <path> <file>` | Signature-only view of one file |
| `contextos watch --root <path>` | Live filesystem watcher, auto-updates the graph |
| `contextos serve --root <path>` | MCP JSON-RPC server on stdio |
| `contextos stats --root <path>` | Graph node / edge / file counts |
| `contextos optimize [--max-tokens N] [--pretty]` | Run the pipeline; stdin → stdout JSON |
| `contextos savings [--top N] [--project <path>] [--no-color]` | Show cumulative token savings dashboard |
| `contextos init [--root <path>] [--skip-build]` | One-shot setup: build graph + wire Claude Code |

---

## MCP tools (for Claude Code / Cursor / etc.)

Wire `contextos serve` into your client's MCP config. Exposed tools:

- `optimize` — run the full pipeline on supplied chunks
- `build_graph` / `update_graph` — index or incrementally refresh
- `impact_radius` — "given these changed files, what else is affected?"
- `skeleton` — signature-only file projection
- `graph_stats` — counts

All return MCP-standard `{ content: [{ type: "text", text: "..." }] }`.

---

## Lossless guarantees

What ContextOS *never* does to your code:

- Paraphrase, summarise, or LLM-rewrite
- Rename identifiers, reorder statements, or change types
- Drop code that's called from the blast radius
- Introduce inferred or hallucinated context

What it *does* do:

- Skip files the call graph says are irrelevant
- Replace bodies with signatures for peripheral files (opt-in via `skeleton_hint`)
- Strip comments / debug logs / redundant whitespace (AST-aware, won't touch string literals)
- Drop exact + near-duplicate chunks
- Drop lowest-ranked chunks only when above the token budget

---

## Testing

```bash
cargo test --workspace                              # unit + integration
npm --workspace apps/extension run compile          # extension type-check
```

Integration tests assert ≥40% reduction on realistic inputs and <200ms elapsed for 50-chunk workloads.

---

## License

MIT
